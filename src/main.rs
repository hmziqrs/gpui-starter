use gpui_starter::{app, events, single_instance};

fn main() {
    let preflight = single_instance::preflight();
    if !preflight.should_start {
        return;
    }
    let startup_runtime = preflight.runtime;
    let startup_deep_link = preflight.initial_deep_link;

    let app_runtime =
        gpui_platform::application().with_assets(gpui_starter::app::assets::CombinedAssets::new());
    app_runtime.run(move |cx| {
        app::init(cx);
        if let Some(runtime) = startup_runtime {
            single_instance::install(runtime, cx);
        }
        if let Some(link) = startup_deep_link {
            events::emit(events::AppEventKind::DeepLinkReceived(link), cx);
        }

        #[cfg(target_os = "macos")]
        gpui_starter::tray::setup(cx);

        cx.activate(true);
        app::create_new_window("My App", cx);
    });

    // After GPUI has fully shut down (the run closure returned), re-exec the
    // binary only when a restart was requested. exec_reload() never returns on
    // success; on failure it logs and we fall through to a normal exit.
    //
    // CAVEAT (needs runtime verification): the SingleInstanceRuntime is held
    // as a GPUI Global inside run(); for exec() to let the relaunched process
    // win preflight(), that lock must be released first. single-instance-0.3.3
    // binds via an abstract Unix socket with no SOCK_CLOEXEC, and exec() does
    // not run Rust dtors, so release depends on the Application (and its
    // globals) being dropped when run() returns. If a restart ever silently
    // no-ops, set FD_CLOEXEC on the lock fd (needs a crate accessor) or
    // spawn-then-exit instead of exec(). See reload.rs.
    #[cfg(unix)]
    {
        #[allow(clippy::collapsible_if)]
        if app::is_reload_requested() {
            if let Err(err) = app::exec_reload() {
                eprintln!("reload failed: {err}");
            }
        }
    }
}
