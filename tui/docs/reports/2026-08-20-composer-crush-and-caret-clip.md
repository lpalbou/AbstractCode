# The composer stopped at 2 rows and typing went blind — chrome-column overflow, and the row that dies is the caret's

Date: 2026-08-20
Investigator seat: abstractcode (framework seat, operator session)
Status of every claim below: **CONFIRMED** by measurement unless marked otherwise.

---

## 1. Headline

Operator report, verbatim: *"the prompt panel at the bottom is limited to 2 lines
(should be 3) and it doesn't follow my text, so if i go over 2 lines, i don't see
what i am writing!"*

Both halves are one defect. The composer requests 4 rows (`TextArea::rows(1, 4)`)
and was being drawn inside a 3-row parent (2 on the operator's terminal). The
`TextArea` itself kept its 4 rows and scrolled correctly — the caret's row was the
fourth — but that fourth row lay outside the parent's rect, and the status bar,
painted after it, overwrote those cells. The row destroyed is therefore always the
one the caret is on, which is exactly why typing past the visible rows was blind.

Root cause is ours and is fixed. A second, engine-side half is filed with the
`tui` seat.

## 2. Root cause

`transcript_view::pane` wraps the transcript in

```rust
dyn_view_scoped(LayoutStyle::column().grow(1.0).padding(Edges::hv(1, 0)), …)
```

with **no flex basis**. An absent basis means AUTO, i.e. measured from content —
the entire transcript. So the chrome column's children demanded hundreds of rows
more than the viewport from the first transcript item onward, and the solver's
shrink pass ran on every frame. Shrink is CSS-scaled (weight = `shrink × basis`),
and among the shrinkable rows the composer had the largest basis (4 against the
1-row chrome lines), so the composer is what paid — exactly one row.

Three measurements pin it:

| Condition | composer rows | caret row visible |
| --- | --- | --- |
| unfixed, 0 transcript items | 4 | yes |
| unfixed, 1 item | 3 | **no** |
| unfixed, 60 items | 3 | **no** |
| unfixed, 300 items | 3 | **no** |
| unfixed, viewport 20 / 30 / 40 / 60 rows | 3 | **no** |
| transcript pane removed from the column | 4 | yes |
| `basis(Cells(0))` on the pane wrapper | 4 | yes |
| `shrink(0.0)` on the composer row | 4 | yes |

Note what the table says: the loss is a CONSTANT one row from a single item on,
independent of transcript length and of terminal height. It is not a
"long-transcript" bug — every non-empty session had it.

Two false leads worth recording, because both look right:

- **The `Scroll`'s basis is not the problem.** `transcript_view` overrides the
  Scroll's layout with `LayoutStyle::default().grow(1.0)`, which silently drops the
  `basis(Cells(0))` the engine puts in `Scroll`'s default (abstracttui backlog 0240
  follow-up #1, `src/widgets/scroll.rs`). Restoring it explicitly changes nothing:
  the pressure comes from the WRAPPER one level up, whose auto basis re-derives a
  content size regardless. The engine's basis-0 default does not survive a wrapper.
- **The `TextArea`'s scroll window is not the problem.** The widget's
  `publish_caret_cell` re-adjusts its window against its solved rect, and its own
  `shrink(0.0)` was honored — it solved to 4 rows and painted the caret on row 4.
  Nothing about the widget's math is wrong. The rows it painted outside its parent
  were simply overpainted afterwards by the next sibling.

## 3. The fix (two one-liners)

1. **Root** — `src/ui/transcript_view.rs:1091`: `basis(Dimension::Cells(0))` beside
   `grow(1.0)` on the pane wrapper. The pane now takes LEFTOVER space instead of
   demanding its content size, so the chrome column no longer overflows at all.
   This is the engine's own `Scroll` default, applied one level up where it was
   being lost.
2. **Guard** — `src/ui/chrome.rs:947`: `shrink(0.0)` on the composer's `Block` row,
   so the row cannot be bought down by whatever pressure appears next. This is what
   the engine's own zero-collapse diagnostic prescribes for a row that must never
   yield.

Deliberately NOT applied: `shrink(0.0)` on the composer's inner column wrapper.
That child sits on the `Block`'s ROW main axis, where shrink means WIDTH — it is
what pulls the TextArea's `width: 100%` basis back to leave the 2-cell `❯` gutter
room. Setting it pushes the right `▌` stroke off screen (tried, reverted).

## 4. Verification

- Regression test `composer_grows_to_four_rows_and_keeps_the_caret_row_visible`
  (`tests/headless_ui.rs`), driven through the real interface with
  `Driver`/`CaptureTerm`: 100x30, 60 transcript items, 8 wrapped rows of draft;
  asserts 4 composer rows, the caret's row (`LINE8`) present and the buffer head
  (`LINE4`) absent — i.e. the window rides the caret. **Verified failing before the
  fix** (3 rows, caret row gone) and passing after.
- Full `cargo test`: green, 0 failures. `cargo clippy --all-targets`: clean apart
  from a pre-existing `items after a test module` in `src/exec.rs:1016`, untouched
  by this work.
- Short terminals (8/10/14 rows): the composer keeps its 4 rows and the status bar
  survives; the transcript pane absorbs. **Bonus**: the engine's
  `layout: fixed-size child … collapsed to 0` notices that used to appear at ≤14
  rows are gone too — they were downstream of the same overflow.

## 5. Filed with the engine (`tui` seat)

`abstracttui/docs/backlog/proposed/first-app/1330_overflowing_child_is_overpainted_by_its_next_sibling.md`
— with a self-contained ~60-line repro. Summary of the ask:

1. An automatic content-based minimum on the main axis (the CSS `min-height: auto`
   analogue) so an ancestor cannot be shorter than a child that refuses to shrink.
   This is `app/popups.rs::floor_declared_size` — already shipped for modals —
   generalized into the solver.
2. Diagnostic parity (the cheap one): the 0240 follow-up #3 notice fires only at
   exactly zero (`src/layout/solve.rs`), so a 4→3 crush and any overflow are
   completely silent. One line of stderr would have made this a two-minute fix
   instead of a bisect.
3. Optionally, honest clipping at the parent's content box, so surplus rows
   truncate visibly instead of landing on a sibling's cells.
4. Regardless: the comment at `src/widgets/textarea.rs:415` claims "shrink 0 so an
   overflowing sibling can never crush the composer". That guarantee covers the
   widget's own box only, and it is what this seat's first diagnosis trusted.

## 6. Things left alone, deliberately

- `chrome::activity_strip` is `line(1)` with default shrink — the same shape of
  vulnerability, and it does vanish on an 8-row terminal. Not what was reported;
  with the pane's basis fixed there is no longer pressure to crush it.
- The redundant `.layout(LayoutStyle::default().grow(1.0))` override on the pane's
  `Scroll` (`transcript_view.rs:1120`) is now inert but still drops the engine's
  basis-0 default. Deleting it would let the engine default apply, which is
  strictly what we want; left in place because it changes nothing today and the
  wrapper fix covers it.
