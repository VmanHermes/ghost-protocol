import { FormEvent } from "react";
import { fmt } from "../api";
import type { Message } from "../types";

type Props = {
  messages: Message[];
  messageInput: string;
  selectedConversationId: string | null;
  activeRunId: string | null;
  actionError: string;
  onChangeMessageInput: (value: string) => void;
  onSendMessage: (event: FormEvent) => void;
  onRetryRun: () => void;
  onCancelRun: () => void;
};

export function ChatView({
  messages,
  messageInput,
  selectedConversationId,
  activeRunId,
  actionError,
  onChangeMessageInput,
  onSendMessage,
  onRetryRun,
  onCancelRun,
}: Props) {
  return (
    <>
      <div className="toolbar main-chat-toolbar">
        <button onClick={onRetryRun} disabled={!activeRunId}>Retry run</button>
        <button onClick={onCancelRun} disabled={!activeRunId}>Cancel run</button>
      </div>
      {actionError ? <div className="error-banner">{actionError}</div> : null}
      <div className="messages">
        {messages.map((message) => (
          <article key={message.id} className={`message ${message.role}`}>
            <header>
              <strong>{message.role}</strong>
              <span>{fmt(message.createdAt)}</span>
            </header>
            <pre>{message.content}</pre>
          </article>
        ))}
      </div>
      <form className="composer" onSubmit={onSendMessage}>
        <textarea
          value={messageInput}
          onChange={(event) => onChangeMessageInput(event.currentTarget.value)}
          placeholder="Send a message to Hermes…"
          rows={4}
        />
        <button type="submit" disabled={!selectedConversationId || !messageInput.trim()}>
          Send and run
        </button>
      </form>
    </>
  );
}
