export type SseEvent = {
  id?: string;
  event?: string;
  data?: string;
};

export class SseParser {
  private _buffer = "";
  private _current: { id?: string; event?: string; data_lines: string[] } = { data_lines: [] };

  push(chunk: string, on_event: (ev: SseEvent) => void): void {
    this._buffer += chunk;

    while (true) {
      const idx = this._buffer.indexOf("\n");
      if (idx === -1) return;
      const raw_line = this._buffer.slice(0, idx);
      this._buffer = this._buffer.slice(idx + 1);

      const line = raw_line.endsWith("\r") ? raw_line.slice(0, -1) : raw_line;
      if (line.startsWith(":")) continue;
      if (line === "") {
        if (this._current.data_lines.length > 0 || this._current.event || this._current.id) {
          const ev: SseEvent = { id: this._current.id, event: this._current.event, data: this._current.data_lines.join("\n") };
          on_event(ev);
        }
        this._current = { data_lines: [] };
        continue;
      }

      const colon = line.indexOf(":");
      const field = colon === -1 ? line : line.slice(0, colon);
      const value = colon === -1 ? "" : line.slice(colon + 1).trimStart();

      if (field === "id") this._current.id = value;
      else if (field === "event") this._current.event = value;
      else if (field === "data") this._current.data_lines.push(value);
    }
  }
}

