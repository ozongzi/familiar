import {
  type KeyboardEvent,
  useCallback,
  useRef,
  useEffect,
  useState,
} from "react";
import styles from "./ChatInput.module.css";

interface Props {
  onSend: (text: string) => void;
  onInterrupt?: (text: string) => void;
  onAbort?: () => void;
  streaming?: boolean;
  disabled?: boolean;
  placeholder?: string;
  token?: string | null;
}

export function ChatInput({
  onSend,
  onInterrupt,
  onAbort,
  streaming = false,
  disabled = false,
  placeholder,
  token,
}: Props) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [hasText, setHasText] = useState(false);
  const [isUploading, setIsUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);

  const MAX_FILE_SIZE = 50 * 1024 * 1024; // 50 MB

  // Auto-resize textarea to fit content
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
      // During streaming: non-empty text → interrupt, empty → abort
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
      // Enter sends/interrupts/aborts; Shift+Enter inserts newline
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        submit();
      }
      // Escape aborts during streaming
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

  // Handle file upload
  const handleFileUpload = useCallback(async () => {
    const fileInput = fileInputRef.current;
    if (!fileInput || !fileInput.files || fileInput.files.length === 0) return;

    const file = fileInput.files[0];

    // Client-side size check before even attempting the upload.
    if (file.size > MAX_FILE_SIZE) {
      setUploadError(`文件过大（最大 50 MB），当前：${(file.size / 1024 / 1024).toFixed(1)} MB`);
      fileInput.value = "";
      return;
    }

    const formData = new FormData();
    formData.append("file", file, file.name);

    setIsUploading(true);
    setUploadError(null);

    try {
      const authToken = token ?? localStorage.getItem("familiar_token");
      const res = await fetch("/api/files", {
        method: "POST",
        headers: {
          Authorization: `Bearer ${authToken}`,
        },
        body: formData,
      });

      if (!res.ok) {
        const err = await res.json().catch(() => ({ error: "上传失败" }));
        throw new Error(err?.error ?? `上传失败 (${res.status})`);
      }

      const json = await res.json();
      console.log("Uploaded:", json);
      // Success — error state already cleared above.
    } catch (err) {
      setUploadError(err instanceof Error ? err.message : "上传失败，请重试");
    } finally {
      // Always reset the input so re-selecting the same file triggers onChange.
      fileInput.value = "";
      setIsUploading(false);
    }
  }, [token, MAX_FILE_SIZE]);

  // Derive what the action button does right now
  const isAbortMode = streaming && !hasText;
  const isInterruptMode = streaming && hasText;
  const isSendMode = !streaming;

  const btnDisabled = disabled || (isSendMode && !hasText);

  return (
    <div className={styles.wrapper}>
      <div className={`${styles.box} ${disabled ? styles.boxDisabled : ""}`}>
        {/* Hidden file input */}
        <input
          ref={fileInputRef}
          type="file"
          className={styles.fileInput}
          onChange={handleFileUpload}
          aria-label="上传文件"
        />

        {/* Upload button */}
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

        {/* Abort button — shown separately when streaming with no text */}
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

        {/* Send / interrupt button */}
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

      {streaming && (
        <p className={styles.hint}>正在生成回复… 可追加消息或按 Esc 打断</p>
      )}
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
      <line x1="22" y1="2" x2="11" y2="13" />
      <polygon points="22 2 15 22 11 13 2 9 22 2" />
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
