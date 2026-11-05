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
/// Files opened in one go ("Open With" on a selection) are collected for this
/// long and handed to a single window, so selecting 50 images does not start 50
/// processes.
const BATCH_WINDOW: Duration = Duration::from_millis(250);

/// What a dock click / "open the app again" should do. Set once at startup so
/// the Objective-C callback below can reach it.
static REOPEN: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
/// Receives the file list of an `openFiles:` (plural) event. Selecting several
/// images in the Finder delivers one event with a list, not one per file.
static OPEN_FILES: OnceLock<Box<dyn Fn(Vec<String>) + Send + Sync>> = OnceLock::new();

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
    // A panic must never unwind into Objective-C.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(action) = REOPEN.get() {
            action();
        }
    }));
    if outcome.is_err() {
        warn!("reopen handler failed");
    }
    true
}

/// `application:openFiles:` - AppKit sends this instead of `openFile:` when more
/// than one document is opened at once (e.g. "Open With" on a selection).
unsafe extern "C" fn objc_open_files(
    _this: &objc::runtime::Object,
    _cmd: objc::runtime::Sel,
    _sender: *mut objc::runtime::Object,
    files: *mut objc::runtime::Object,
) {
    use objc::{msg_send, sel, sel_impl};

    if files.is_null() {
        return;
    }

    let count: usize = msg_send![files, count];
    let mut paths = Vec::with_capacity(count.min(256));
    for index in 0..count {
        let item: *mut objc::runtime::Object = msg_send![files, objectAtIndex: index];
        if item.is_null() {
            continue;
        }
        let path = fruitbasket::nsstring_to_string(item);
        if !path.is_empty() {
            paths.push(path);
        }
    }

    if paths.is_empty() {
        return;
    }

    info!("Opening {} documents from a single event", paths.len());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(action) = OPEN_FILES.get() {
            action(paths);
        }
    }));
    if outcome.is_err() {
        warn!("openFiles handler failed");
    }
}

/// Add the reopen handler to the running NSApplication delegate.
///
/// fruitbasket only creates a fixed set of delegate methods, and a `rapp` Apple
/// Event is consumed by NSApplication before the event manager sees it, so the
/// method is injected into the delegate class at runtime instead.
fn install_delegate_handlers(
    reopen: Box<dyn Fn() + Send + Sync>,
    open_files: Box<dyn Fn(Vec<String>) + Send + Sync>,
) {
    use objc::runtime::{class_addMethod, object_getClass, Class, Imp, Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};

    if REOPEN.set(reopen).is_err() {
        return;
    }
    if OPEN_FILES.set(open_files).is_err() {
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
        // BOOL is a C++ bool on arm64 and a signed char on x86_64.
        let types: &[u8] = if cfg!(target_arch = "aarch64") {
            b"B@:@B\0"
        } else {
            b"c@:@c\0"
        };
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

        let files_imp: Imp = std::mem::transmute(
            objc_open_files
                as unsafe extern "C" fn(&Object, Sel, *mut Object, *mut Object),
        );
        let files_added = class_addMethod(
            cls,
            Sel::register("application:openFiles:"),
            files_imp,
            b"v@:@@\0".as_ptr() as *const std::os::raw::c_char,
        );
        debug!("openFiles handler installed: {files_added}");
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
                // Several paths are valid: a window can show a whole selection.
                .multiple_values(true)
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
    // Documents are collected briefly and opened together (see BATCH_WINDOW).
    let pending_files: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let batch_scheduled = Arc::new(AtomicBool::new(false));

    let mut app = FruitApp::new();
    // The helper itself must not show up in the dock, it owns no window.
    app.set_activation_policy(ActivationPolicy::Accessory);

    // A plain launch (no document): give AppKit a moment, then open one window
    // unless a document arrived in the meantime.
    {
        let children = children.clone();
        let spawned_any = spawned_any.clone();
        let document_seen = document_seen.clone();
        // Clones for this closure; the originals stay available below.
        let pending_files = pending_files.clone();
        let batch_scheduled = batch_scheduled.clone();
        app.register_callback(
            FruitCallbackKey::Method("applicationDidFinishLaunching:"),
            Box::new(move |_event| {
                // The delegate only exists once the application finished
                // launching; installing earlier finds a nil delegate.
                {
                    let children = children.clone();
                    let spawned_any = spawned_any.clone();
                    install_delegate_handlers(
                        Box::new({
                            let children = children.clone();
                            let spawned_any = spawned_any.clone();
                            move || {
                                if spawn_window(&[], &children) {
                                    spawned_any.store(true, Ordering::SeqCst);
                                }
                            }
                        }),
                        Box::new({
                            let pending = pending_files.clone();
                            let scheduled = batch_scheduled.clone();
                            let children = children.clone();
                            let spawned_any = spawned_any.clone();
                            move |files: Vec<String>| {
                                queue_files(&files, &pending, &scheduled, &children, &spawned_any);
                            }
                        }),
                    );
                }

                let children = children.clone();
                let spawned_any = spawned_any.clone();
                let document_seen = document_seen.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(LAUNCH_GRACE);
                    if !document_seen.swap(true, Ordering::SeqCst) {
                        info!("No document at launch, opening an empty window");
                        if spawn_window(&[], &children) {
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
        let pending_files = pending_files.clone();
        let batch_scheduled = batch_scheduled.clone();
        app.register_callback(
            FruitCallbackKey::Method("application:openFile:"),
            Box::new(move |file| {
                let path = fruitbasket::nsstring_to_string(file);
                info!("Received {path}");
                document_seen.store(true, Ordering::SeqCst);
                if path.is_empty() {
                    warn!("openFile without a path, ignoring");
                    return;
                }
                queue_files(
                    &[path],
                    &pending_files,
                    &batch_scheduled,
                    &children,
                    &spawned_any,
                );
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

/// Add documents to the pending batch and make sure a window is opened for them.
fn queue_files(
    files: &[String],
    pending: &Arc<Mutex<Vec<String>>>,
    scheduled: &Arc<AtomicBool>,
    children: &Arc<Mutex<Vec<Child>>>,
    spawned_any: &Arc<AtomicBool>,
) {
    let Ok(mut list) = pending.lock() else {
        warn!("pending file list is poisoned");
        return;
    };
    list.extend(files.iter().cloned());
    drop(list);
    schedule_batch(pending, scheduled, children, spawned_any);
}

/// Collect documents for [`BATCH_WINDOW`] and open them in one window.
fn schedule_batch(
    pending: &Arc<Mutex<Vec<String>>>,
    scheduled: &Arc<AtomicBool>,
    children: &Arc<Mutex<Vec<Child>>>,
    spawned_any: &Arc<AtomicBool>,
) {
    if scheduled.swap(true, Ordering::SeqCst) {
        return;
    }
    let pending = pending.clone();
    let scheduled = scheduled.clone();
    let children = children.clone();
    let spawned_any = spawned_any.clone();
    std::thread::spawn(move || {
        std::thread::sleep(BATCH_WINDOW);
        let files = match pending.lock() {
            Ok(mut list) => std::mem::take(&mut *list),
            Err(_) => Vec::new(),
        };
        scheduled.store(false, Ordering::SeqCst);
        if files.is_empty() {
            return;
        }
        if spawn_window(&files, &children) {
            spawned_any.store(true, Ordering::SeqCst);
        }
    });
}

/// Spawn one window (a separate process) showing `files` and remember it.
fn spawn_window(files: &[String], children: &Arc<Mutex<Vec<Child>>>) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        warn!("Cannot determine the executable path");
        return false;
    };

    let mut cmd = Command::new(exe);
    for file in files {
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
