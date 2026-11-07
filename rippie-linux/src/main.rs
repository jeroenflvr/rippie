use std::{
    collections::HashMap,
    env, fmt, fs,
    io::{self, Read},
    os::fd::AsFd,
    path::Path,
    process,
};

use wayland_client::{
    backend::ObjectId,
    event_created_child,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry::WlRegistry, wl_seat::WlSeat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self as device, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self as offer, ZwlrDataControlOfferV1},
};

// One entry per format, candidates in order of preference.
const FORMATS: &[&[&str]] = &[
    &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"],
    &["text/html"],
    &["text/rtf", "application/rtf"],
    &["image/png", "image/tiff", "image/jpeg", "image/gif"],
];

#[derive(Default)]
struct State {
    offers: HashMap<ObjectId, (ZwlrDataControlOfferV1, Vec<String>)>,
    selection: Option<ObjectId>,
    changed: bool,
    finished: bool,
}

fn main() {
    let Some(dir) = env::args().nth(1) else {
        eprintln!("usage: rippie-linux <shared-dir>");
        process::exit(2);
    };
    let dir = Path::new(&dir).join(".rippie");
    fs::create_dir_all(&dir)
        .unwrap_or_else(|e| fail(format_args!("create {}: {e}", dir.display())));
    let path = dir.join("clipboard");
    let tmp = dir.join(".clipboard.tmp");

    let conn = Connection::connect_to_env().unwrap_or_else(|e| fail(e));
    let (globals, mut queue) = registry_queue_init::<State>(&conn).unwrap_or_else(|e| fail(e));
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=1, ()).unwrap_or_else(|e| fail(e));
    let manager: ZwlrDataControlManagerV1 = globals
        .bind(&qh, 1..=2, ())
        .unwrap_or_else(|e| fail(format_args!("compositor lacks wlr-data-control: {e}")));
    manager.get_data_device(&seat, &qh, ());

    let mut state = State::default();
    let mut last = fs::read(&path).unwrap_or_default();
    loop {
        queue
            .blocking_dispatch(&mut state)
            .unwrap_or_else(|e| fail(e));
        if state.finished {
            fail("data-control device finished");
        }
        if !std::mem::take(&mut state.changed) {
            continue;
        }
        let Some((offer, mimes)) = state.selection.as_ref().and_then(|id| state.offers.get(id))
        else {
            continue;
        };
        let snapshot = match read_offer(&conn, offer, mimes) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                eprintln!("rippie-linux: read clipboard: {e}");
                continue;
            }
        };
        if snapshot.is_empty() || snapshot == last {
            continue;
        }
        // Write-then-rename so the mac side never sees a partial buffer.
        match fs::write(&tmp, &snapshot).and_then(|_| fs::rename(&tmp, &path)) {
            Ok(()) => last = snapshot,
            Err(e) => eprintln!("rippie-linux: write {}: {e}", path.display()),
        }
    }
}

fn fail(msg: impl fmt::Display) -> ! {
    eprintln!("rippie-linux: {msg}");
    process::exit(1);
}

/// Reads each supported format from the offer as records of `<mime>\n<len>\n<bytes>`.
fn read_offer(
    conn: &Connection,
    offer: &ZwlrDataControlOfferV1,
    mimes: &[String],
) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    for candidates in FORMATS {
        let Some(mime) = candidates.iter().find(|c| mimes.iter().any(|m| m == *c)) else {
            continue;
        };
        let (mut reader, writer) = io::pipe()?;
        offer.receive(mime.to_string(), writer.as_fd());
        conn.flush().map_err(io::Error::other)?;
        drop(writer);
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        if data.is_empty() {
            continue;
        }
        out.extend_from_slice(format!("{mime}\n{}\n", data.len()).as_bytes());
        out.extend_from_slice(&data);
    }
    Ok(out)
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            device::Event::DataOffer { id } => {
                state.offers.insert(id.id(), (id, Vec::new()));
            }
            device::Event::Selection { id } => {
                let keep = id.map(|o| o.id());
                state.offers.retain(|k, (o, _)| {
                    let current = keep.as_ref() == Some(k);
                    if !current {
                        o.destroy();
                    }
                    current
                });
                state.selection = keep;
                state.changed = true;
            }
            device::Event::PrimarySelection { id: Some(o) } => {
                if let Some((o, _)) = state.offers.remove(&o.id()) {
                    o.destroy();
                }
            }
            device::Event::Finished => state.finished = true,
            _ => {}
        }
    }

    event_created_child!(State, ZwlrDataControlDeviceV1, [
        device::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwlrDataControlOfferV1,
        event: offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let offer::Event::Offer { mime_type } = event {
            if let Some((_, mimes)) = state.offers.get_mut(&proxy.id()) {
                mimes.push(mime_type);
            }
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: <ZwlrDataControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
