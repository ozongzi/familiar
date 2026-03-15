import {
  type KeyboardEvent,
  useCallback,
  useRef,
  useEffect,
  useState,
} from "react";
import styles from "./ChatInput.module.css";

interface UploadResult {
  filename: string;
  path: string;
  size: number;
}

interface Props {
  onSend: (text: string) => void;
  onInterrupt?: (text: string) => void;
  onAbort?: () => void;
  streaming?: boolean;
  disabled?: boolean;
  placeholder?: string;
  token?: string | null;
  conversationId?: string | null;
  requestConversationId?: () => Promise<string | null>;
  onUpload?: (result: UploadResult) => void;
  onOpenMcp?: () => void;
}

export function ChatInput({
  onSend,
  onInterrupt,
  onAbort,
  streaming = false,
  disabled = false,
  placeholder,
  token,
  conversationId,
  requestConversationId,
  onUpload,
  onOpenMcp,
}: Props) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [hasText, setHasText] = useState(false);
  const [isUploading, setIsUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const MAX_FILE_SIZE = 50 * 1024 * 1024;

  const resize = useCallback(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, []);

  useEffect(() => {
    resize();
  }, [resize]);

  const submit = useCallback(() => {
    const el = textareaRef.current;
    if (!el || disabled) return;
    const text = el.value.trim();
    if (streaming) {
      if (text && onInterrupt) {
        onInterrupt(text);
        el.value = "";
        el.style.height = "auto";
        setHasText(false);
      } else if (!text && onAbort) {
        onAbort();
      }
      return;
    }
    if (!text) return;
    onSend(text);
    el.value = "";
    el.style.height = "auto";
    setHasText(false);
  }, [onSend, onInterrupt, onAbort, streaming, disabled]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        submit();
      }
      if (e.key === "Escape" && streaming && onAbort) {
        onAbort();
      }
    },
    [submit, streaming, onAbort],
  );

  const handleInput = useCallback(() => {
    resize();
    const el = textareaRef.current;
    setHasText((el?.value.trim().length ?? 0) > 0);
  }, [resize]);

  const handleFileUpload = useCallback(async () => {
    const fileInput = fileInputRef.current;
    if (!fileInput || !fileInput.files || fileInput.files.length === 0) return;
    const file = fileInput.files[0];
    if (file.size > MAX_FILE_SIZE) {
      setUploadError(
        `文件过大（最大 50 MB），当前：${(file.size / 1024 / 1024).toFixed(1)} MB`,
      );
      fileInput.value = "";
      return;
    }
    let convId = conversationId ?? null;
    if (!convId && requestConversationId) {
      convId = await requestConversationId();
    }
    const formData = new FormData();
    formData.append("file", file, file.name);
    if (convId) formData.append("conversation_id", convId);
    setIsUploading(true);
    setUploadError(null);
    try {
      const authToken = token ?? localStorage.getItem("familiar_token");
      const res = await fetch("/api/files", {
        method: "POST",
        headers: { Authorization: `Bearer ${authToken}` },
        body: formData,
      });
      if (!res.ok) {
        const err = await res.json().catch(() => ({ error: "上传失败" }));
        throw new Error(err?.error ?? `上传失败 (${res.status})`);
      }
      const json = (await res.json()) as {
        filename: string;
        path: string;
        size: number;
      };
      onUpload?.({ filename: json.filename, path: json.path, size: json.size });
    } catch (err) {
      setUploadError(err instanceof Error ? err.message : "上传失败，请重试");
    } finally {
      fileInput.value = "";
      setIsUploading(false);
    }
  }, [token, conversationId, requestConversationId, onUpload, MAX_FILE_SIZE]);

  const isAbortMode = streaming && !hasText;
  const isInterruptMode = streaming && hasText;
  const isSendMode = !streaming;
  const btnDisabled = disabled || (isSendMode && !hasText);

  return (
    <div className={styles.wrapper}>
      <div className={`${styles.box} ${disabled ? styles.boxDisabled : ""}`}>
        <input
          ref={fileInputRef}
          type="file"
          className={styles.fileInput}
          onChange={handleFileUpload}
          aria-label="上传文件"
        />

        {/* 上方：textarea */}
        <textarea
          ref={textareaRef}
          className={styles.textarea}
          placeholder={
            placeholder ??
            (streaming
              ? "追加消息… (Enter 发送，Esc 打断)"
              : "发消息… (Enter 发送，Shift+Enter 换行)")
          }
          rows={1}
          onInput={handleInput}
          onKeyDown={handleKeyDown}
          disabled={disabled}
          aria-label="消息输入框"
        />

        {/* 下方：工具栏 */}
        <div className={styles.toolbar}>
          <div className={styles.toolbarLeft}>
            <button
              className={`${styles.uploadBtn} ${isUploading ? styles.uploadBtnLoading : ""}`}
              onClick={() => fileInputRef.current?.click()}
              disabled={disabled || isUploading}
              aria-label="上传文件"
              title="上传文件"
            >
              {isUploading ? (
                <span className={styles.uploadSpinner} />
              ) : (
                <UploadIcon />
              )}
            </button>

            {onOpenMcp && (
              <button
                className={styles.uploadBtn}
                onClick={onOpenMcp}
                aria-label="MCP 服务器"
                title="MCP 服务器"
              >
                <PlugIcon />
              </button>
            )}
          </div>

          <div className={styles.toolbarRight}>
            {streaming && <span className={styles.hint}>正在生成…</span>}

            {isAbortMode && (
              <button
                className={styles.abortBtn}
                onClick={onAbort}
                aria-label="停止生成"
                title="停止生成 (Esc)"
              >
                <StopIcon />
              </button>
            )}

            {!isAbortMode && (
              <button
                className={`${styles.sendBtn} ${isInterruptMode ? styles.sendBtnInterrupt : ""}`}
                onClick={submit}
                disabled={btnDisabled}
                aria-label={isInterruptMode ? "追加消息" : "发送"}
                title={isInterruptMode ? "追加消息 (Enter)" : "发送 (Enter)"}
              >
                {isInterruptMode ? <InterruptIcon /> : <SendIcon />}
              </button>
            )}
          </div>
        </div>
      </div>

      {uploadError && (
        <p className={styles.uploadError} role="alert">
          ⚠️ {uploadError}
        </p>
      )}
    </div>
  );
}

function SendIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden="true"
    >
      <path d="M2 12L22 2L13 22L11 13L2 12Z" />
    </svg>
  );
}

function StopIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden="true"
    >
      <rect x="4" y="4" width="16" height="16" rx="2" />
    </svg>
  );
}

function InterruptIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polyline points="13 17 18 12 13 7" />
      <polyline points="6 17 11 12 6 7" />
    </svg>
  );
}

function UploadIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="17 8 12 3 7 8" />
      <line x1="12" y1="3" x2="12" y2="15" />
    </svg>
  );
}

function PlugIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M18 7l-1.5-1.5" />
      <path d="M6 7l1.5-1.5" />
      <path d="M12 2v2" />
      <rect x="4" y="7" width="16" height="8" rx="2" />
      <path d="M12 17v3" />
      <path d="M9 20h6" />
    </svg>
  );
}
