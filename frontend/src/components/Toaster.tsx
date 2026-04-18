import { useToasts } from "../store/toast";
import styles from "./Toaster.module.css";

export function Toaster() {
  const toasts = useToasts();
  if (!toasts.length) return null;
  return (
    <div className={styles.container}>
      {toasts.map((t) => (
        <div key={t.id} className={`${styles.toast} ${styles[t.type]}`}>
          {t.message}
        </div>
      ))}
    </div>
  );
}
