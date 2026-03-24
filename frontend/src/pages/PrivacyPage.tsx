import styles from "./PrivacyPage.module.css";

export function PrivacyPage() {
  return (
    <div className={styles.page}>
      <div className={styles.content}>
        <h1>隐私政策</h1>
        <p className={styles.updated}>最后更新：2026 年 3 月</p>

        <section>
          <h2>1. 我们收集的信息</h2>
          <p>当您使用 Familiar 时，我们会收集以下信息：</p>
          <ul>
            <li><strong>账号信息：</strong>通过 GitHub OAuth 授权获取的用户名、头像等基本信息</li>
            <li><strong>对话内容：</strong>您与 AI 助手的聊天记录，用于提供连续对话功能</li>
            <li><strong>上传文件：</strong>您主动上传的文件内容</li>
            <li><strong>使用数据：</strong>服务使用情况的统计数据（如 Token 用量）</li>
          </ul>
        </section>

        <section>
          <h2>2. 信息的使用方式</h2>
          <p>我们使用收集的信息用于：</p>
          <ul>
            <li>提供、维护和改进 AI 助手服务</li>
            <li>响应您的请求和提供技术支持</li>
            <li>监控服务的使用情况以防止滥用</li>
          </ul>
          <p>我们<strong>不会</strong>将您的个人信息或对话内容用于模型训练、广告定向或其他商业目的。</p>
        </section>

        <section>
          <h2>3. 信息共享</h2>
          <p>我们不会出售、出租或共享您的个人信息给第三方，以下情况除外：</p>
          <ul>
            <li>经您明确同意</li>
            <li>法律法规要求</li>
            <li>为提供服务所必需的基础设施服务商（受保密协议约束）</li>
          </ul>
          <p>您的对话内容会通过 AI 模型 API 处理，请参阅相关模型提供商的隐私政策。</p>
        </section>

        <section>
          <h2>4. 数据存储与安全</h2>
          <p>您的数据存储在我们的服务器上，我们采取合理的技术措施保护您的信息安全。会话令牌存储在您的浏览器本地，不会传输给第三方。</p>
        </section>

        <section>
          <h2>5. 您的权利</h2>
          <p>您有权：</p>
          <ul>
            <li>访问您的个人信息</li>
            <li>更正不准确的信息</li>
            <li>删除您的账号及相关数据</li>
            <li>撤回对数据处理的同意</li>
          </ul>
          <p>如需行使上述权利，请通过以下方式联系我们。</p>
        </section>

        <section>
          <h2>6. 联系我们</h2>
          <p>如您对本隐私政策有任何疑问，请联系服务管理员。</p>
        </section>
      </div>
    </div>
  );
}
