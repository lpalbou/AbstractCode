import { useState } from "react";
import { AfTimeZonePicker, type TimeZonePreference } from "@abstractframework/ui-kit";

/** Settings → Workflow: the ACCOUNT's time zone kept by the gateway (round 16, R16.1 A2) — the
 * kit AfTimeZonePicker over the gateway's IANA list, "Gateway default (<zone>)" first. New
 * automations run their daily / weekly / monthly / one-time rules in it; the schedule dialog only
 * shows it ("Change in preferences" opens this panel). A change is one PUT at once (no Save). */
export function AccountTimeZone(props: {
  block: TimeZonePreference;
  save: (value: string | null) => Promise<unknown>;
  disabled?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  return (
    <div className="code-account-time-zone" data-account-time-zone>
      <AfTimeZonePicker
        id="code-account-time-zone"
        block={props.block}
        disabled={props.disabled || busy}
        note={note}
        onChange={async (next) => {
          setBusy(true);
          setNote(null);
          try {
            await props.save(next);
            setNote({ ok: true, text: "Saved." });
          } catch (reason) {
            setNote({ ok: false, text: `Not saved. ${reason instanceof Error ? reason.message : String(reason)}` });
          } finally {
            setBusy(false);
          }
        }}
      />
    </div>
  );
}
