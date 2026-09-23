import { describe, expect, it, vi } from 'vitest';
import { buildWorkflowInput } from './catalog';
import { GatewayClient } from '../lib/gateway_client';
import { DEFAULT_PREFERENCES } from './settings_panel';

describe('MTP application transport', () => {
  it('inherits unless explicitly set, preserving Off', () => {
    expect(DEFAULT_PREFERENCES.speculation).toBeUndefined();
    const off = buildWorkflowInput({speculation:false});
    expect((off._runtime as any).speculation).toBe(false);
    expect(off.speculation).toBeUndefined();
    const depth = {mode:'native_mtp' as const,num_draft_tokens:4,require_acceleration:true as const};
    expect((buildWorkflowInput({speculation:depth})._runtime as any).speculation).toEqual(depth);
  });
  it('discovers the backend as well as the model without a mutation', async () => {
    const fetch = vi.fn().mockResolvedValue({ok:true,json:async()=>({execution:{}})});
    vi.stubGlobal('fetch', fetch);
    try {
      await new GatewayClient({base_url:''}).discovery_model_capabilities('org/model', 'endpoint:local');
      expect(fetch.mock.calls[0][0]).toContain('model_name=org%2Fmodel');
      expect(fetch.mock.calls[0][0]).toContain('provider=endpoint%3Alocal');
      expect(fetch.mock.calls[0][1].method).toBeUndefined();
    } finally { vi.unstubAllGlobals(); }
  });
});
