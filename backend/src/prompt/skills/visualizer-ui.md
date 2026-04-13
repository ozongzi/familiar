---
name: visualizer-ui
description: UI组件和界面原型规范，包括卡片、表单、数据记录、选项对比等布局模式和设计token。
---

## UI 组件

### 美学
扁平、干净、白色表面。最小的 0.5px 边框。充足的留白。无渐变、无阴影（除功能性聚焦环外）。一切都应感觉是 claude.ai 的原生组件——就像它本来就属于页面，而非从别处嵌入。

### 设计令牌
- 边框：始终 `0.5px solid var(--color-border-tertiary)`（强调时用 `-secondary`）
- 圆角：大多数元素用 `var(--border-radius-md)`，卡片用 `var(--border-radius-lg)`
- 卡片：白色背景（`var(--color-background-primary)`），0.5px 边框，radius-lg，内边距 1rem 1.25rem
- 表单元素（`input`、`select`、`textarea`、`button`、`range slider`）已预置样式——写入裸标签。文本输入框为 36px，内置悬停/聚焦效果；范围滑块有 4px 轨道 + 18px 滑块；按钮具有轮廓样式，带悬停/激活效果。仅添加内联样式以覆盖（例如不同的宽度）。
- 按钮：预置样式，透明背景，0.5px border-secondary，悬停时 bg-secondary，激活时 scale(0.98)。如果触发 `sendPrompt`，追加 `↗` 箭头。
- **每个显示的数字都要四舍五入。** JS 浮点运算会泄漏伪影——`0.1 + 0.2` 给出 `0.30000000000000004`，`7 * 1.1` 给出 `7.700000000000001`。任何出现在屏幕上的数字（滑块读数、统计卡片值、轴标签、数据点标签、工具提示、计算总数）都必须通过 `Math.round()`、`.toFixed(n)` 或 `Intl.NumberFormat`。根据上下文选择合适的精度——计数用整数，百分比用 1–2 位小数，货币用 `toLocaleString()`。对于范围滑块，也设置 `step="1"`（或 `step="0.1"` 等），以便输入本身发出舍入值。
- 间距：垂直节奏用 `rem`（1rem、1.5rem、2rem），组件内部间隙用 `px`（8px、12px、16px）
- 阴影：无，除了输入框上的 `box-shadow: 0 0 0 Npx` 聚焦环

### 指标卡片
用于汇总数字（收入、计数、百分比）——表面卡片，上方为柔和的 13px 标签，下方为 24px/500 数字。`background: var(--color-background-secondary)`，无边框，`border-radius: var(--border-radius-md)`，内边距 1rem。用于 2-4 个网格中，`gap: 12px`。与浮起卡片（白色背景 + 边框）区分开。

### 布局
- 编辑式（解释性内容）：无卡片包裹，文字自然流动
- 卡片（有界对象，如联系人记录、收据）：单个浮起卡片包裹整个内容
- 不要将表格放在这里——在回复文本中以 Markdown 形式输出

**网格溢出：** `grid-template-columns: 1fr` 默认 `min-width: auto`——具有较大 `min-content` 的子项会将列推出容器。使用 `minmax(0, 1fr)` 来约束。

**表格溢出：** 具有多列的表格如果单元格内容超出，会自动扩展超出 `width: 100%`。在受限布局（≤700px）中，使用 `table-layout: fixed` 并设置明确的列宽，或减少列数，或在包装器上允许水平滚动。

### 线框图展示
内含的线框图——移动屏幕、聊天线程、单个卡片、模态框、小型 UI 组件——应放置在背景表面（`var(--color-background-secondary)` 容器，带 `border-radius: var(--border-radius-lg)` 和内边距，或设备框架）上，这样它们就不会赤裸地悬浮在小部件画布上。全宽线框图如仪表板、设置页面或自然填满视口的数据表则不需要额外的包装器。

### 1. 交互式解释器 — 学习某物如何工作
*“解释复利如何运作” / “教我排序算法”*

使用 `imagine_html` 处理交互式控件——滑块、按钮、实时状态显示、图表。将文字解释保留在正常的回复文本中（工具调用之外），不要嵌入 HTML。无卡片包裹。留白即是容器。

```html
<div style="display: flex; align-items: center; gap: 12px; margin: 0 0 1.5rem;">
  <label style="font-size: 14px; color: var(--color-text-secondary);">Years</label>
  <input type="range" min="1" max="40" value="20" id="years" style="flex: 1;" />
  <span style="font-size: 14px; font-weight: 500; min-width: 24px;" id="years-out">20</span>
</div>

<div style="display: flex; align-items: baseline; gap: 8px; margin: 0 0 1.5rem;">
  <span style="font-size: 14px; color: var(--color-text-secondary);">£1,000 →</span>
  <span style="font-size: 24px; font-weight: 500;" id="result">£3,870</span>
</div>

<div style="margin: 2rem 0; position: relative; height: 240px;">
  <canvas id="chart"></canvas>
</div>
```

使用 `sendPrompt()` 让用户提出后续问题：`sendPrompt('如果我提高到 10% 的利率会怎样？')`

### 2. 选项比较 — 决策制定
*“比较这些产品的定价和功能” / “帮我在 React 和 Vue 之间选择”*

使用 `imagine_html`。选项的并排卡片网格。用语义颜色突出显示差异。用于过滤或加权的交互式元素。

- 使用 `repeat(auto-fit, minmax(160px, 1fr))` 实现响应式列
- 每个选项放在一张卡片中。使用徽章表示关键差异点。
- 添加 `sendPrompt()` 按钮：`sendPrompt('告诉我更多关于专业版的信息')`
- 不要将比较表格放入此工具——改为在回复文本中以常规 Markdown 表格输出。工具仅用于视觉卡片网格。
- 当推荐某个选项或“最受欢迎”时，仅用 `border: 2px solid var(--color-border-info)` 强调其卡片（2px 是故意的——这是 0.5px 规则的唯一例外，用于强调特色项目）——保持与其他卡片相同的背景和边框。在卡片头部上方或内部添加一个小徽章（例如“最受欢迎”），使用 `background: var(--color-background-info); color: var(--color-text-info); font-size: 12px; padding: 4px 12px; border-radius: var(--border-radius-md)`。

### 3. 数据记录 — 有界 UI 对象
*“给我展示一个 Salesforce 联系人卡片” / “为这个订单创建收据”*

使用 `imagine_html`。将整个内容包裹在一个浮起卡片中。所有内容均为无衬线字体，因为它是纯 UI。对人使用头像/首字母圆圈（见下方示例）。

```html
<div style="background: var(--color-background-primary); border-radius: var(--border-radius-lg); border: 0.5px solid var(--color-border-tertiary); padding: 1rem 1.25rem;">
  <div style="display: flex; align-items: center; gap: 12px; margin-bottom: 16px;">
    <div style="width: 44px; height: 44px; border-radius: 50%; background: var(--color-background-info); display: flex; align-items: center; justify-content: center; font-weight: 500; font-size: 14px; color: var(--color-text-info);">MR</div>
    <div>
      <p style="font-weight: 500; font-size: 15px; margin: 0;">Maya Rodriguez</p>
      <p style="font-size: 13px; color: var(--color-text-secondary); margin: 0;">VP of Engineering</p>
    </div>
  </div>
  <div style="border-top: 0.5px solid var(--color-border-tertiary); padding-top: 12px;">
    <table style="width: 100%; font-size: 13px;">
      <tr><td style="color: var(--color-text-secondary); padding: 4px 0;">Email</td><td style="text-align: right; padding: 4px 0; color: var(--color-text-info);">m.rodriguez@acme.com</td></tr>
      <tr><td style="color: var(--color-text-secondary); padding: 4px 0;">Phone</td><td style="text-align: right; padding: 4px 0;">+1 (415) 555-0172</td></tr>
    </table>
  </div>
</div>
```
