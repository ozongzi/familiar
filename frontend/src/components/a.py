with open("MessageBubble.tsx", "r") as f:
    content = f.read()

old = '''function WidgetChatBubble({ bubble }: { bubble: ToolBubble }) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(200);
  const [visible, setVisible] = useState(false);

  // Track the last srcdoc length we wrote so we only re-write on change
  const docOpenedRef = useRef(false);
  const lastSrcdocRef = useRef("");

  const srcdoc = useMemo(
    () => (bubble.widgetCode ? buildWidgetSrcdoc(bubble.widgetCode) : ""),
    [bubble.widgetCode],
  );

  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe || !srcdoc) return;
    // Skip if content hasn\'t changed
    if (srcdoc === lastSrcdocRef.current) return;
    lastSrcdocRef.current = srcdoc;

    const doc = iframe.contentDocument;
    if (!doc) return;

    if (!docOpenedRef.current) {
      doc.open();
      docOpenedRef.current = true;
      setTimeout(() => setVisible(true), 30);
    }

    // Always write the full document prefix from the start.
    // The browser\'s incremental HTML parser will render whatever is already
    // well-formed, while leaving the rest pending \u2014 no truncated-tag artifacts.
    doc.write(srcdoc);

    if (!bubble.pending) {
      doc.close();
      docOpenedRef.current = false;
    }
  }, [srcdoc, bubble.pending]);

  // Height tracking: postMessage from widget + RAF poll fallback
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe) return;

    const onMessage = (e: MessageEvent) => {
      if (e.source !== iframe.contentWindow) return;
      if (
        e.data?.type === "familiar-widget-height" &&
        typeof e.data.height === "number"
      ) {
        setHeight(Math.min(Math.max(e.data.height, 60), 2000));
      }
    };
    window.addEventListener("message", onMessage);

    let raf: number;
    let prevH = 0;
    const poll = () => {
      try {
        const doc = iframe.contentDocument;
        if (doc?.body) {
          const h = doc.body.scrollHeight;
          if (h > 20 && h !== prevH) {
            prevH = h;
            setHeight(Math.min(Math.max(h, 60), 2000));
          }
        }
      } catch {
        // cross-origin \u2014 ignore
      }
      raf = requestAnimationFrame(poll);
    };
    raf = requestAnimationFrame(poll);

    return () => {
      window.removeEventListener("message", onMessage);
      cancelAnimationFrame(raf);
    };
  }, []);

  const loadingMsgs = bubble.widgetLoadingMessages?.length
    ? bubble.widgetLoadingMessages
    : null;
  const [msgIdx, setMsgIdx] = useState(0);
  useEffect(() => {
    if (!bubble.pending || !loadingMsgs) return;
    const t = setInterval(() => setMsgIdx((i) => (i + 1) % loadingMsgs.length), 2200);
    return () => clearInterval(t);
  }, [bubble.pending, loadingMsgs]);

  if (!srcdoc && !bubble.pending) return null;

  return (
    <div className={styles.row} style={{ justifyContent: "flex-start" }}>
      <div className={styles.widgetBubble}>
        {bubble.pending && loadingMsgs && !visible && (
          <div className={styles.widgetLoading}>
            <span className={styles.widgetLoadingDot} />
            <span className={styles.widgetLoadingText}>{loadingMsgs[msgIdx]}</span>
          </div>
        )}
        <iframe
          ref={iframeRef}
          sandbox="allow-scripts allow-same-origin"
          className={styles.widgetIframe}
          style={{
            height: visible ? height : 0,
            opacity: visible ? 1 : 0,
            transition: "opacity 0.25s ease",
          }}
          title="widget"
        />
      </div>
    </div>
  );
}'''

new = '''function WidgetChatBubble({ bubble }: { bubble: ToolBubble }) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(200);
  const [visible, setVisible] = useState(false);
  const [msgIdx, setMsgIdx] = useState(0);

  const loadingMsgs = bubble.widgetLoadingMessages?.length
    ? bubble.widgetLoadingMessages
    : ["\u751f\u6210\u4e2d\u2026"];

  // Only render once streaming is done
  const srcdoc = useMemo(
    () => !bubble.pending && bubble.widgetCode ? buildWidgetSrcdoc(bubble.widgetCode) : "",
    [bubble.widgetCode, bubble.pending],
  );

  // Push srcdoc to iframe imperatively to avoid React recreating the element
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe || !srcdoc) return;
    iframe.srcdoc = srcdoc;
    setTimeout(() => setVisible(true), 50);
  }, [srcdoc]);

  // Cycle loading messages while pending
  useEffect(() => {
    if (!bubble.pending) return;
    const t = setInterval(() => setMsgIdx((i) => (i + 1) % loadingMsgs.length), 2200);
    return () => clearInterval(t);
  }, [bubble.pending, loadingMsgs.length]);

  // Height: postMessage + RAF poll
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe) return;
    const onMessage = (e: MessageEvent) => {
      if (e.source !== iframe.contentWindow) return;
      if (e.data?.type === "familiar-widget-height" && typeof e.data.height === "number")
        setHeight(Math.min(Math.max(e.data.height, 60), 2000));
    };
    window.addEventListener("message", onMessage);
    let raf: number;
    let prevH = 0;
    const poll = () => {
      try {
        const d = iframe.contentDocument;
        if (d?.body) {
          const h = d.body.scrollHeight;
          if (h > 20 && h !== prevH) { prevH = h; setHeight(Math.min(Math.max(h, 60), 2000)); }
        }
      } catch { /* cross-origin */ }
      raf = requestAnimationFrame(poll);
    };
    raf = requestAnimationFrame(poll);
    return () => { window.removeEventListener("message", onMessage); cancelAnimationFrame(raf); };
  }, []);

  if (!bubble.pending && !srcdoc) return null;

  return (
    <div className={styles.row} style={{ justifyContent: "flex-start" }}>
      <div className={styles.widgetBubble}>
        {bubble.pending && (
          <div className={styles.widgetLoading}>
            <span className={styles.widgetLoadingDot} />
            <span key={msgIdx} className={styles.widgetLoadingText}>{loadingMsgs[msgIdx]}</span>
          </div>
        )}
        <iframe
          ref={iframeRef}
          sandbox="allow-scripts allow-same-origin"
          className={styles.widgetIframe}
          style={{ height, opacity: visible ? 1 : 0, transition: "opacity 0.25s ease" }}
          title="widget"
        />
      </div>
    </div>
  );
}'''

assert old in content, "NOT FOUND"
content = content.replace(old, new, 1)
with open("MessageBubble.tsx", "w") as f:
    f.write(content)
print("Done")
