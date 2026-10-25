//! The status-line chip "Automations · N waiting" (R17.1; rules in
//! `crate::attention`). Enter on an empty prompt, or a click on the chip,
//! opens the automation that waits.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use abstracttui::prelude::*;
use abstracttui::reactive::IntervalHandle;

use crate::attention;
use crate::runner::Cmd;
use crate::store::{Conn, Store};
use crate::ui::UiCtx;

/// The listed automations (empty until the gateway answered).
fn listed(store: Store, tracked: bool) -> Vec<crate::automations::Summary> {
    let read = |v: &crate::automations::View| {
        v.list
            .as_ref()
            .and_then(|l| l.as_ref().ok())
            .cloned()
            .unwrap_or_default()
    };
    if tracked {
        store.automations.with(read)
    } else {
        store.automations.with_untracked(read)
    }
}

/// The chip's text (tracked: call inside a render); `None` = no chip.
/// From the person's event stream while it is followed; from the list on a
/// gateway without it.
pub fn chip_text(store: Store) -> Option<String> {
    if store.events_state.get() == attention::EventsState::Following {
        return store
            .event_attention
            .with(|a| attention::chip_events(a.needs_you(), a.new_results()));
    }
    attention::chip(attention::waiting_total(&listed(store, true)))
}

/// Open the automation that waits. Returns false when nothing waits.
pub fn open_waiting(cx: Scope, store: Store, ctx: &UiCtx) -> bool {
    match attention::first_waiting(&listed(store, false)) {
        Some(id) => {
            crate::ui::automations_view::open_automation(cx, store, ctx, &id);
            true
        }
        None => false,
    }
}

/// One attention read (the list + the definitions not read yet).
pub fn read(store: Store, ctx: &UiCtx) {
    let known = store.automation_ask.with_untracked(|a| a.revisions());
    ctx.send(Cmd::AutomationAttention { known });
}

pub fn wire(cx: Scope, store: Store, ctx: UiCtx) {
    // One read on every reachable + signed-in edge (boot, gateway back).
    {
        let ctx = ctx.clone();
        let up = Cell::new(false);
        cx.effect(move || {
            let now = store.conn.get() == Conn::Ok && store.signed_out.with(Option::is_none);
            if now && !up.get() {
                read(store, &ctx);
                // The person's event stream (a newer follower retires the old).
                ctx.send(Cmd::FollowEvents);
            }
            up.set(now);
        });
    }
    // An automation event on the stream re-reads the list (coalesced: a
    // burst of events is one read), so Enter opens what the chip counts.
    {
        let ctx = ctx.clone();
        let last = Cell::new(0u64);
        let pending = Rc::new(Cell::new(false));
        cx.effect(move || {
            let epoch = store.event_attention.with(|a| a.automation_epoch);
            let following = store.events_state.get() == attention::EventsState::Following;
            if epoch == last.get() || !following {
                last.set(epoch);
                return;
            }
            last.set(epoch);
            if pending.replace(true) {
                return;
            }
            let ctx = ctx.clone();
            let pending = pending.clone();
            abstracttui::reactive::after(std::time::Duration::from_millis(500), move || {
                pending.set(false);
                read(store, &ctx);
            });
        });
    }
    // The light poll, ONLY for a gateway without the event stream: armed
    // while something waits or an active automation asks before each tool
    // call; cancelled otherwise (the engine's zero-wakeup idle rule).
    let handle: Rc<RefCell<Option<IntervalHandle>>> = Rc::default();
    cx.effect(move || {
        let poll = store.events_state.get() != attention::EventsState::Following && {
            let items = listed(store, true);
            store
                .automation_ask
                .with(|a| attention::should_poll(&items, a))
        };
        let mut slot = handle.borrow_mut();
        match (poll, slot.is_some()) {
            (true, false) => {
                let ctx = ctx.clone();
                *slot = Some(abstracttui::reactive::interval(
                    cx,
                    attention::POLL,
                    move || {
                        // `/automations` re-reads the list itself while open.
                        if !store.automations.with_untracked(|v| v.loading) {
                            read(store, &ctx);
                        }
                    },
                ));
            }
            (false, true) => {
                if let Some(h) = slot.take() {
                    h.cancel();
                }
            }
            _ => {}
        }
    });
}
