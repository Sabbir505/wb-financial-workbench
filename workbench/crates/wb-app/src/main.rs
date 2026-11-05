//! WB — financial-report event-driven analysis & review workbench (GPUI desktop).
//!
//! Usage:
//!   wb-app                 # normal desktop app
//!   wb-app --selfcheck     # open the window, verify render pipeline, exit 0
//!   wb-app --selfcheck-e2e # drive the full review loop through the live UI state
//!
//! Config: wb.config.json next to the exe or CWD; env overrides
//! WB_API_BASE / WB_MCP_URL / WB_VAULT / WB_DATA / WB_WEBHOOK_PORT / WB_REVIEWER.

mod detail;
mod i18n;
mod input;
mod selfcheck;
mod theme;
mod workspace;

use gpui::{
    px, App, AppContext, Application, Bounds, KeyBinding, TitlebarOptions, WindowBounds,
    WindowOptions,
};
use workspace::{ViewKind, Workspace};

fn view_arg(args: &[String]) -> Option<ViewKind> {
    let i = args.iter().position(|a| a == "--view")?;
    match args.get(i + 1).map(|s| s.as_str()) {
        Some("inbox") => Some(ViewKind::Inbox),
        Some("library") => Some(ViewKind::Library),
        Some("quadrant") => Some(ViewKind::Quadrant),
        Some("audit") => Some(ViewKind::AuditTrail),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let selfcheck = args.iter().any(|a| a == "--selfcheck");
    let selfcheck_e2e = args.iter().any(|a| a == "--selfcheck-e2e");
    let initial_view = view_arg(&args);

    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("ctrl-r", workspace::RefreshQueue, None),
            KeyBinding::new("escape", workspace::GoBack, None),
            KeyBinding::new("ctrl-enter", workspace::SubmitReview, None),
            KeyBinding::new("ctrl-1", workspace::ShowInbox, None),
            KeyBinding::new("ctrl-2", workspace::ShowLibrary, None),
            KeyBinding::new("ctrl-3", workspace::ShowQuadrant, None),
            KeyBinding::new("ctrl-4", workspace::ShowAudit, None),
        ]);
        let bounds = Bounds::centered(
            None,
            gpui::Size {
                width: px(1440.0),
                height: px(860.0),
            },
            cx,
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(gpui::Size {
                width: px(1180.0),
                height: px(680.0),
            }),
            titlebar: Some(TitlebarOptions {
                title: Some("WB 财报审核工作台".into()),
                ..Default::default()
            }),
            focus: true,
            show: true,
            ..Default::default()
        };
        let handle = cx
            .open_window(options, |window, cx| {
                cx.new(|cx| Workspace::new(initial_view, window, cx))
            })
            .expect("failed to open WB window");
        cx.activate(true);

        if selfcheck || selfcheck_e2e {
            cx.spawn(async move |cx| {
                if selfcheck_e2e {
                    selfcheck::drive_review_loop(handle, cx).await;
                    // Hold the window briefly so external capture/inspection can run.
                    let final_hold = std::env::var("WB_HOLD_FINAL_MS")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(15_000);
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(final_hold))
                        .await;
                } else {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(4))
                        .await;
                    println!("SELFCHECK_OK: window rendered for 4s without panics");
                }
                cx.update(|cx| cx.quit()).ok();
            })
            .detach();
        }
    });
}
