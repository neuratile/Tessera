/* eslint-disable */
import type { WsEvent } from '@testing-ide/shared';

const MIN_RECONNECT_MS = 1_000;
const MAX_RECONNECT_MS = 30_000;

/**
 * Manages a single WebSocket connection to the Tessera boards server.
 *
 * Features:
 * - Auto-reconnect with exponential backoff (1 s → 30 s cap)
 * - Typed event dispatching via `onEvent` / unsubscribe
 * - Clean disconnect for component unmount
 */
export class BoardWebSocket {
  private ws: WebSocket | null = null;
  private handlers: ((event: WsEvent) => void)[] = [];
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectDelay = MIN_RECONNECT_MS;
  private boardId: string | null = null;
  private serverUrl: string | null = null;
  private token: string | null = null;
  private tokenProvider: (() => Promise<string | null>) | null = null;
  private intentionalClose = false;

  /**
   * Open a WebSocket connection to the boards server.
   *
   * Pass `tokenProvider` to fetch a fresh access token on every (re)connect —
   * a static token goes stale after Supabase's hourly auto-refresh, which
   * would fail the auth handshake on any reconnect after ~60 minutes.
   */
  connect(
    serverUrl: string,
    token: string,
    boardId: string,
    tokenProvider?: () => Promise<string | null>,
  ): void {
    // Tear down any existing connection first
    this.disconnect();

    this.serverUrl = serverUrl;
    this.token = token;
    this.tokenProvider = tokenProvider ?? null;
    this.boardId = boardId;
    this.intentionalClose = false;
    this.reconnectDelay = MIN_RECONNECT_MS;

    this.open();
  }

  /** Cleanly close the connection and stop reconnection attempts. */
  disconnect(): void {
    this.intentionalClose = true;
    this.clearReconnectTimer();

    if (this.ws) {
      this.ws.onopen = null;
      this.ws.onmessage = null;
      this.ws.onerror = null;
      this.ws.onclose = null;
      this.ws.close();
      this.ws = null;
    }

    this.boardId = null;
    this.serverUrl = null;
    this.token = null;
    this.tokenProvider = null;
  }

  /**
   * Register an event handler. Returns an unsubscribe function.
   *
   * ```ts
   * const unsub = boardWs.onEvent((e) => console.log(e));
   * // later:
   * unsub();
   * ```
   */
  onEvent(handler: (event: WsEvent) => void): () => void {
    this.handlers.push(handler);
    return () => {
      this.handlers = this.handlers.filter((h) => h !== handler);
    };
  }

  /** Whether the underlying WebSocket is currently open. */
  get isConnected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  // ── Private helpers ──────────────────────────────────────────────

  private open(): void {
    if (!this.serverUrl || !this.boardId) return;

    // Convert http(s) to ws(s). The token is deliberately NOT a query
    // parameter — URLs end up in access logs, history, and Referer headers.
    // Auth happens via the first message after the connection opens.
    const wsBase = this.serverUrl.replace(/^http/, 'ws');
    const url = `${wsBase}/ws/boards/${this.boardId}`;

    this.ws = new WebSocket(url);

    this.ws.onopen = () => {
      // Reset backoff on successful connection
      this.reconnectDelay = MIN_RECONNECT_MS;
      // First-message auth: the server closes the socket unless this arrives
      // within its auth timeout. Fetch a fresh token when a provider is set —
      // the stored one may have expired between reconnects.
      void this.sendAuth();
    };

    this.ws.onmessage = (msg) => {
      try {
        const event: WsEvent = JSON.parse(msg.data as string);
        if ((event as { type?: string }).type === 'auth_ok') {
          return; // Handshake ack, not a board event
        }
        for (const handler of this.handlers) {
          handler(event);
        }
      } catch {
        // Silently ignore malformed messages
      }
    };

    this.ws.onerror = () => {
      // `onerror` is always followed by `onclose`, so reconnect logic
      // lives in `onclose` only.
    };

    this.ws.onclose = () => {
      this.ws = null;
      if (!this.intentionalClose) {
        this.attemptReconnect();
      }
    };
  }

  private async sendAuth(): Promise<void> {
    let token = this.token ?? '';
    if (this.tokenProvider) {
      try {
        const fresh = await this.tokenProvider();
        if (fresh) {
          this.token = fresh;
          token = fresh;
        }
      } catch {
        // Fall back to the stored token; the server rejects it if expired
        // and the reconnect loop will retry.
      }
    }
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: 'auth', token }));
    }
  }

  private attemptReconnect(): void {
    this.clearReconnectTimer();

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.open();
    }, this.reconnectDelay);

    // Exponential backoff, capped
    this.reconnectDelay = Math.min(this.reconnectDelay * 2, MAX_RECONNECT_MS);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }
}

/** App-wide singleton WebSocket instance. */
export const boardWs = new BoardWebSocket();
