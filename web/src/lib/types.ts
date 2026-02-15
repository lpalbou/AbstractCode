export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export type AttachmentRef = {
  $artifact: string;
  filename?: string;
  path?: string;
  content_type?: string;
  size_bytes?: number;
  sha256?: string;
  [k: string]: any;
};

export type StepRecord = {
  run_id?: string;
  step_id?: string;
  node_id?: string;
  status?: string;
  effect?: {
    type?: string;
    payload?: any;
    result_key?: string;
  };
  result?: any;
  error?: any;
  started_at?: string | null;
  ended_at?: string | null;
  actor_id?: string | null;
  session_id?: string | null;
};

export type LedgerStreamEvent = {
  cursor: number;
  record: StepRecord;
};

export type WaitState = {
  reason?: string;
  wait_key?: string;
  resume_to_node?: string | null;
  result_key?: string | null;
  until?: string | null;
  prompt?: string | null;
  choices?: any[];
  allow_free_text?: boolean;
  details?: any;
};

export type ToolCall = {
  call_id?: string;
  id?: string;
  name?: string;
  arguments?: any;
};

