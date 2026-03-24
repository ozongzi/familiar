import { useState } from "react";
import styles from "./PrivacyConsentModal.module.css";

export function PrivacyConsentModal({ onAccept, onDecline }: { onAccept: () => void; onDecline: () => void }) {
  const [checked, setChecked] = useState(false);

  return (
    <div className={styles.overlay}>
      <div className={styles.modal}>
        <h2 className={styles.title}>欢迎使用 Familiar</h2>
        <p className={styles.subtitle}>在继续之前，请阅读并同意我们的隐私政策</p>

        <div className={styles.policyBox}>
          <h3>隐私政策摘要</h3>
          <p>我们收集以下信息以提供服务：</p>
          <ul>
            <li>GitHub 账号的基本信息（用户名、头像）</li>
            <li>您在平台上的对话内容</li>
            <li>您上传的文件</li>
          </ul>
          <p>我们承诺：</p>
          <ul>
            <li>仅将您的数据用于提供 AI 助手服务</li>
            <li>不会将您的个人信息出售或共享给第三方</li>
            <li>您可以随时联系我们删除您的账号及数据</li>
          </ul>
          <p className={styles.fullLink}>
            完整隐私政策见 <a href="/privacy" target="_blank" rel="noopener noreferrer">/privacy</a>
          </p>
        </div>

        <label className={styles.checkRow}>
          <input
            type="checkbox"
            checked={checked}
            onChange={e => setChecked(e.target.checked)}
          />
          <span>我已阅读并同意《隐私政策》</span>
        </label>

        <div className={styles.btnRow}>
          <button className={styles.btnDecline} onClick={onDecline}>拒绝并退出</button>
          <button className={styles.btnAccept} disabled={!checked} onClick={onAccept}>同意并继续</button>
        </div>
      </div>
    </div>
  );
}
