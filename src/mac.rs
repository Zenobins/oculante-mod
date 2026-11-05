use clap::Arg;
use fruitbasket::ActivationPolicy;
use fruitbasket::FruitApp;
use fruitbasket::FruitCallbackKey;
use fruitbasket::RunPeriod;
use log::{debug, info, warn};
use std::error::Error;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long to wait for a document that arrives together with the launch.
const LAUNCH_GRACE: Duration = Duration::from_millis(400);
/// How often the helper checks whether its windows are still alive.
const CHILD_POLL: Duration = Duration::from_millis(1500);

/// What a dock click / "open the app again" should do. Set once at startup so
/// the Objective-C callback below can reach it.
static REOPEN: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// `applicationShouldHandleReopen:hasVisibleWindows:` for the delegate class
/// fruitbasket installs. Reopening must always produce a new window, because
/// this helper owns no window of its own to bring forward.
unsafe extern "C" fn objc_reopen(
    _this: &objc::runtime::Object,
    _cmd: objc::runtime::Sel,
    _sender: *mut objc::runtime::Object,
    _has_visible: bool,
) -> bool {
    info!("Reopen requested, opening another window");
    if let Some(action) = REOPEN.get() {
        action();
    }
    true
}

/// Add the reopen handler to the running NSApplication delegate.
///
/// fruitbasket only creates a fixed set of delegate methods, and a `rapp` Apple
/// Event is consumed by NSApplication before the event manager sees it, so the
/// method is injected into the delegate class at runtime instead.
fn install_reopen_handler(action: Box<dyn Fn() + Send + Sync>) {
    use objc::runtime::{class_addMethod, object_getClass, Class, Imp, Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};

    if REOPEN.set(action).is_err() {
        return;
    }

    unsafe {
        let nsapp: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        if nsapp.is_null() {
            warn!("No NSApplication, reopen will not open a window");
            return;
        }
        let delegate: *mut Object = msg_send![nsapp, delegate];
        if delegate.is_null() {
            warn!("No application delegate, reopen will not open a window");
            return;
        }
        let cls = object_getClass(delegate) as *mut Class;
        let types = b"B@:@B\0";
        let imp: Imp = std::mem::transmute(
            objc_reopen
                as unsafe extern "C" fn(&Object, Sel, *mut Object, bool) -> bool,
        );
        let added = class_addMethod(
            cls,
            Sel::register("applicationShouldHandleReopen:hasVisibleWindows:"),
            imp,
            types.as_ptr() as *const std::os::raw::c_char,
        );
        debug!("reopen handler installed: {added}");
    }
}

/// macOS integration for launching and for files opened from the Finder.
///
/// This process is only a small helper ("agent"): it owns no window and spawns
/// one child process per window. Keeping it alive is what makes opening a second
/// image from the Finder work at all - macOS sends those open requests as Apple
/// Events to the running application, and a window process (winit) does not
/// listen for them. Upstream never solved this, see woelper/oculante#33.
pub fn launch() -> Result<(), Box<dyn Error>> {
    info!("Starting MacOS integration");

    // It's not good design that the MacOS workaround does its own argument parsing again,
    // However, the notan (and possibly other engines/libraries) structure prefer argument parsing
    // in an init funcion, from which fruitbasked panics internally. For this reason this extra
    // module keeps everything self-contained and barebones so it can be called independently
    // early on.

    info!("Mac: Now matching arguments {:?}", std::env::args());
    // Filter out strange mac args
    let args: Vec<String> = std::env::args().filter(|a| !a.contains("psn_")).collect();
    let matches = clap::Command::new("Oculante")
        .arg(
            Arg::new("INPUT")
                .help("Display this image")
                // .required(true)
                .index(1),
        )
        .arg(
            Arg::new("chainload")
                .required(false)
                .takes_value(false)
                .short('c')
                .help("Chainload on Mac"),
        )
        .get_matches_from(args);

    debug!("Completed argument parsing.");
    let maybe_img_location = matches.get_one::<String>("INPUT").map(PathBuf::from);

    if !matches.is_present("chainload") && maybe_img_location.is_none() {
        info!("Chainload not specified, and no input file present. Invoking mac hack.");
    } else {
        return Ok(());
    }

    let children: Arc<Mutex<Vec<Child>>> = Arc::new(Mutex::new(Vec::new()));
    let spawned_any = Arc::new(AtomicBool::new(false));
    // Set as soon as a document arrives, so a plain launch does not open an
    // additional empty window on top of it.
    let document_seen = Arc::new(AtomicBool::new(false));

    let mut app = FruitApp::new();
    // The helper itself must not show up in the dock, it owns no window.
    app.set_activation_policy(ActivationPolicy::Accessory);

    // A plain launch (no document): give AppKit a moment, then open one window
    // unless a document arrived in the meantime.
    {
        let children = children.clone();
        let spawned_any = spawned_any.clone();
        let document_seen = document_seen.clone();
        app.register_callback(
            FruitCallbackKey::Method("applicationDidFinishLaunching:"),
            Box::new(move |_event| {
                // The delegate only exists once the application finished
                // launching; installing earlier finds a nil delegate.
                {
                    let children = children.clone();
                    let spawned_any = spawned_any.clone();
                    install_reopen_handler(Box::new(move || {
                        if spawn_window(None, &children) {
                            spawned_any.store(true, Ordering::SeqCst);
                        }
                    }));
                }

                let children = children.clone();
                let spawned_any = spawned_any.clone();
                let document_seen = document_seen.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(LAUNCH_GRACE);
                    if !document_seen.swap(true, Ordering::SeqCst) {
                        info!("No document at launch, opening an empty window");
                        if spawn_window(None, &children) {
                            spawned_any.store(true, Ordering::SeqCst);
                        }
                    }
                });
            }),
        );
    }

    // Files opened from the Finder or with `open -a`, at launch and later on.
    // This is the callback that was missing while the app was already running.
    {
        let children = children.clone();
        let spawned_any = spawned_any.clone();
        let document_seen = document_seen.clone();
        app.register_callback(
            FruitCallbackKey::Method("application:openFile:"),
            Box::new(move |file| {
                let path = fruitbasket::nsstring_to_string(file);
                info!("Received {path}");
                document_seen.store(true, Ordering::SeqCst);
                if spawn_window(Some(path), &children) {
                    spawned_any.store(true, Ordering::SeqCst);
                }
            }),
        );
    }

    // The helper only exists to serve its windows; once the last one is gone it
    // terminates so no invisible process lingers around.
    {
        let children = children.clone();
        let spawned_any = spawned_any.clone();
        let stopper = app.stopper();
        std::thread::spawn(move || {
            let mut empty_polls = 0;
            let mut waited = Duration::ZERO;
            loop {
                std::thread::sleep(CHILD_POLL);
                waited += CHILD_POLL;

                if !spawned_any.load(Ordering::SeqCst) {
                    // No window was ever created (spawn failed): do not linger.
                    if waited > Duration::from_secs(15) {
                        warn!("No window could be opened, stopping the macOS helper");
                        stopper.stop();
                        break;
                    }
                    continue;
                }

                let alive = {
                    let mut list = children.lock().unwrap();
                    list.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
                    !list.is_empty()
                };
                if alive {
                    empty_polls = 0;
                    continue;
                }

                // Two empty polls in a row: guards against a window that is
                // being spawned while the list is checked.
                empty_polls += 1;
                if empty_polls >= 2 {
                    info!("All windows closed, stopping the macOS helper");
                    stopper.stop();
                    break;
                }
            }
        });
    }

    let _ = app.run(RunPeriod::Forever);

    fruitbasket::FruitApp::terminate(0);

    // This will never execute.
    Ok(())
}

/// Spawn one window (a separate process) and remember it.
fn spawn_window(file: Option<String>, children: &Arc<Mutex<Vec<Child>>>) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        warn!("Cannot determine the executable path");
        return false;
    };

    let mut cmd = Command::new(exe);
    if let Some(file) = &file {
        cmd.arg(file);
    }
    // `-c` marks a window process: it must not start its own helper again.
    cmd.arg("-c");

    match cmd.spawn() {
        Ok(child) => {
            if let Ok(mut list) = children.lock() {
                list.push(child);
            }
            true
        }
        Err(e) => {
            warn!("Could not start a window: {e}");
            false
        }
    }
}
