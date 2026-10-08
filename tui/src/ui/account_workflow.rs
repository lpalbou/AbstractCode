//! The account's "Default for new conversations" in the terminal (R17.1;
//! model and wording in `crate::account_prefs`, wire in
//! `crate::gateway::preferences`).
//!
//! - read when the gateway is reachable and signed in (boot, and again after
//!   it comes back), with the one-time upload of this computer's old choice;
//! - the old choice is removed from this computer once the gateway settled
//!   it (`clear_device`); `/workflow` then picks THIS conversation's
//!   workflow only and saves nothing on this computer;
//! - a fresh conversation (no run yet) starts on the account's default once
//!   the answer and the workflow list are in, and follows a change of it
//!   (the web's `accountDefaultKey` rule); `/new` starts on it too;
//! - on a gateway without the route, nothing changes: `/workflow` keeps
//!   saving the choice on this computer, as before.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use abstracttui::prelude::*;

use crate::account_prefs::{self as ap, State};
use crate::gateway::preferences::PrefCmd;
use crate::runner::Cmd;
use crate::store::{Conn, Store, Workflow};
use crate::ui::UiCtx;

/// The old device choice as an account value (`None` = gateway default).
fn device_value(ctx: &UiCtx) -> Option<String> {
    let p = ctx.prefs.borrow();
    let (b, f) = p.workflow_preference();
    ap::device_value(b.as_deref(), f.as_deref())
}

/// Read the row (once per reachable + signed-in edge).
pub fn load(store: Store, ctx: &UiCtx) {
    store.account_workflow.update(|v| {
        if !matches!(v.state, State::Ok(_)) {
            v.state = State::Loading;
        }
    });
    ctx.send(Cmd::AccountPrefs(PrefCmd::Load {
        device: device_value(ctx),
    }));
}

/// One change: ONE PUT (no Save button), the row busy until it answers.
pub fn save(store: Store, ctx: &UiCtx, value: Option<String>) {
    if store.account_workflow.with_untracked(|v| v.busy) {
        return;
    }
    store.account_workflow.update(|v| {
        v.busy = true;
        v.note = None;
    });
    ctx.send(Cmd::AccountPrefs(PrefCmd::Save { value }));
}

/// Whether the gateway keeps the default (the device choice is ignored).
pub fn managed(store: Store) -> bool {
    store.account_workflow.with_untracked(ap::View::managed)
}

/// The workflow the account's default runs, from the terminal's catalog:
/// the listed `(bundle, flow)`, else the gateway default (`None` value, or
/// a value the catalog does not hold — the row says why).
pub fn default_workflow(
    row: &ap::Row,
    workflows: &[Workflow],
    gateway_default: Option<Workflow>,
) -> Option<Workflow> {
    if let Some((b, f)) = row.workflow() {
        if let Some(w) = workflows
            .iter()
            .find(|w| w.bundle_id == b && w.flow_id == f)
        {
            return Some(w.clone());
        }
    }
    gateway_default
}

/// Put a FRESH conversation (no run yet) on the account's default.
/// Returns whether it changed the selection.
pub fn apply_to_fresh(store: Store) -> bool {
    if !store.run_id.get_untracked().is_empty() {
        return false;
    }
    let Some(row) = store.account_workflow.with_untracked(|v| v.row().cloned()) else {
        return false;
    };
    let workflows = store.workflows.get_untracked();
    let Some(w) = default_workflow(
        &row,
        &workflows,
        store.gateway_default_workflow.get_untracked(),
    ) else {
        return false;
    };
    let same = store.workflow.with_untracked(|cur| {
        cur.bundle_id == w.bundle_id
            && cur.flow_id == w.flow_id
            && cur.gateway_default == w.gateway_default
    });
    if !same {
        if !w.supports_gating() {
            store.gating_mode.set(String::new());
        }
        store.workflow.set(w);
    }
    !same
}

/// What a fresh conversation follows: (account value, list size, default known).
type DefaultKey = (Option<String>, usize, bool);

pub fn wire(cx: Scope, store: Store, ctx: UiCtx) {
    // Read on every reachable + signed-in edge (boot, gateway back).
    {
        let ctx = ctx.clone();
        let up = Cell::new(false);
        cx.effect(move || {
            let now = store.conn.get() == Conn::Ok && store.signed_out.with(Option::is_none);
            if now && !up.get() {
                load(store, &ctx);
            }
            up.set(now);
        });
    }
    // The migration settled: remove the old choice from this computer.
    {
        let ctx = ctx.clone();
        cx.effect(move || {
            if store.account_workflow.with(|v| v.clear_device) {
                store.account_workflow.update(|v| v.clear_device = false);
                crate::ui::persist_prefs(&ctx, |p| p.set_gateway_default_workflow());
            }
        });
    }
    // A fresh conversation follows the account default (value, list).
    {
        let last: Rc<RefCell<Option<DefaultKey>>> = Rc::default();
        cx.effect(move || {
            let value = store
                .account_workflow
                .with(|v| v.row().map(|r| r.value.clone()));
            let Some(value) = value else { return };
            let key = (
                value,
                store.workflows.with(Vec::len),
                store.gateway_default_workflow.with(Option::is_some),
            );
            if last.borrow().as_ref() == Some(&key) {
                return;
            }
            *last.borrow_mut() = Some(key);
            untrack(|| apply_to_fresh(store));
        });
    }
}
