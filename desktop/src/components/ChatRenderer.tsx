import { useCallback, useEffect, useRef, useState } from "react";
import type { ChatMessage } from "../types";

type Props = {
  messages: ChatMessage[];
  streamingDelta: string;
  streamingMessageId: string | null;
  status: string;
  onSendMessage: (content: string) => void;
};

export function ChatRenderer({ messages, streamingDelta, streamingMessageId: _streamingMessageId, status, onSendMessage }: Props) {
  const [draft, setDraft] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const isEnded = status === "exited" || status === "terminated" || status === "error";

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingDelta]);

  const handleSend = useCallback(() => {
    if (isEnded) return;
    const content = draft.trim();
    if (!content) return;
    onSendMessage(content);
    setDraft("");
  }, [draft, isEnded, onSendMessage]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); }
  }, [handleSend]);

  return (
    <div className="chat-renderer">
      <div className="chat-messages">
        {messages.map((msg) => (
          <div key={msg.id} className={`chat-bubble chat-bubble-${msg.role}`}>
            {msg.role === "system" ? (
              <div className="chat-system-msg">{msg.content}</div>
            ) : (
              <>
                <div className="chat-bubble-header">
                  <span className="chat-bubble-role">{msg.role === "user" ? "You" : "Assistant"}</span>
                </div>
                <div className="chat-bubble-content">{msg.content}</div>
              </>
            )}
          </div>
        ))}
        {streamingDelta && (
          <div className="chat-bubble chat-bubble-assistant chat-bubble-streaming">
            <div className="chat-bubble-header">
              <span className="chat-bubble-role">Assistant</span>
              <span className="chat-streaming-indicator">●</span>
            </div>
            <div className="chat-bubble-content">{streamingDelta}</div>
          </div>
        )}
        {status === "thinking" && !streamingDelta && <div className="chat-status-indicator">Thinking...</div>}
        {status === "tool_use" && <div className="chat-status-indicator">Using tool...</div>}
        {status === "exited" && <div className="chat-status-indicator">Session ended.</div>}
        {status === "terminated" && <div className="chat-status-indicator">Session terminated.</div>}
        {status === "error" && !streamingDelta && <div className="chat-status-indicator">Session ended with an error.</div>}
        <div ref={messagesEndRef} />
      </div>
      <div className="chat-composer">
        <textarea
          ref={textareaRef}
          className="chat-input"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={isEnded ? "This session has ended." : "Send a message..."}
          rows={1}
          disabled={isEnded}
        />
        <button className="btn-primary chat-send-btn" onClick={handleSend} disabled={isEnded || !draft.trim()}>Send</button>
      </div>
    </div>
  );
}
