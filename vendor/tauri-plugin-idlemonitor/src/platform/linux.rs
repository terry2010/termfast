use tauri::{AppHandle, Emitter, Runtime};

use crate::platform::types::LockListener;

pub fn start_lock_listener<R: Runtime>(app: &AppHandle<R>) -> std::result::Result<LockListener, String> {
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_clone = running.clone();

    let app_clone = app.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime for linux lock listener");

        rt.block_on(listen_dbus(&app_clone, running_clone));
    });

    Ok(LockListener {
        stop: Box::new(move || {
            running.store(false, std::sync::atomic::Ordering::Relaxed);
        }),
    })
}

async fn listen_dbus<R: Runtime>(
    app: &AppHandle<R>,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    // zbus 5.x removed the public add_match API. Instead, we use a Proxy to
    // listen for specific signals. However, the simplest approach that works
    // across zbus versions is to use MessageStream and filter manually.
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[idlemonitor] failed to connect to DBus session bus: {e}");
            return;
        }
    };

    // Add match rules via raw method call (AddMatch is a standard DBus method).
    let add_match = |rule: &str| async {
        use zbus::zvariant::Value;
        let _ = conn
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "AddMatch",
                &(rule,),
            )
            .await;
    };

    add_match("type='signal',interface='org.freedesktop.ScreenSaver',member='ActiveChanged'").await;
    add_match("type='signal',interface='org.freedesktop.login1.Manager',member='PrepareForSleep'").await;

    let mut stream = zbus::MessageStream::from(&conn);

    loop {
        if !running.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        use futures_util::StreamExt;
        let msg = match tokio::time::timeout(
            std::time::Duration::from_secs(1),
            stream.next(),
        ).await {
            Ok(Some(Ok(m))) => m,
            _ => continue,
        };

        let header = msg.header();
        let iface = header.interface().map(|s| s.as_str());
        let member = header.member().map(|s| s.as_str());

        match (iface, member) {
            (Some("org.freedesktop.ScreenSaver"), Some("ActiveChanged")) => {
                // zbus 5.x: body() takes no generic argument; use deserialize.
                let body = msg.body();
                if let Ok(active) = body.deserialize::<bool>() {
                    let _ = app.emit("system:lock", crate::error::LockPayload { locked: active });
                } else if let Ok(v) = body.deserialize::<zbus::zvariant::Value<'_>>() {
                    if let Ok(active) = <bool as TryFrom<zbus::zvariant::Value<'_>>>::try_from(v) {
                        let _ = app.emit("system:lock", crate::error::LockPayload { locked: active });
                    }
                }
            }
            (Some("org.freedesktop.login1.Manager"), Some("PrepareForSleep")) => {
                if let Ok(sleeping) = msg.body().deserialize::<bool>() {
                    if sleeping {
                        let _ = app.emit("system:suspend", crate::error::SuspendPayload {});
                    } else {
                        let _ = app.emit("system:resume", crate::error::ResumePayload {});
                    }
                }
            }
            _ => {}
        }
    }
}
