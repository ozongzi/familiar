---
name: visualizer-chart
description: 数据图表规范，包括Chart.js配置、图例、数字格式化，以及D3 Choropleth地理地图的拓扑数据源和投影设置。
---

## 图表（Chart.js）
```html
<div style="position: relative; width: 100%; height: 300px;">
  <canvas id="myChart"></canvas>
</div>
<script src="https://cdnjs.cloudflare.com/ajax/libs/Chart.js/4.4.1/chart.umd.js"></script>
<script>
  new Chart(document.getElementById('myChart'), {
    type: 'bar',
    data: { labels: ['Q1','Q2','Q3','Q4'], datasets: [{ label: 'Revenue', data: [12,19,8,15] }] },
    options: { responsive: true, maintainAspectRatio: false }
  });
</script>
```

**Chart.js 规则**：
- Canvas 无法解析 CSS 变量。使用硬编码十六进制值或 Chart.js 默认值。
- 将 `<canvas>` 包裹在具有明确 `height` 和 `position: relative` 的 `<div>` 中。
- **Canvas 尺寸**：仅在外层 div 上设置高度，绝不在 canvas 元素本身上设置。在包装器上使用 `position: relative`，在 Chart.js 选项中使用 `responsive: true, maintainAspectRatio: false`。切勿直接在 canvas 上设置 CSS 高度——这会导致尺寸错误，尤其是水平条形图。
- 对于水平条形图：外层 div 高度应至少为 `(条数 * 40) + 80` 像素。
- 通过 `<script src="https://cdnjs.cloudflare.com/ajax/libs/...">` 加载 UMD 构建版本——设置 `window.Chart` 全局变量。后跟普通 `<script>`（无 `type="module"`）。
- 多个图表：使用唯一 ID（`myChart1`、`myChart2`）。每个图表有自己的一对 canvas+div。
- 对于气泡图和散点图：气泡半径会超出其中心点，因此靠近轴边界的点会被裁剪。将比例范围扩大——将 `scales.y.min` 和 `scales.y.max` 设置为超出数据范围约 10%（x 轴同理）。或使用 `layout: { padding: 20 }` 作为粗略的后备方案。
- Chart.js 在标签会重叠时自动跳过 x 轴标签。如果你有 ≤12 个类别且需要显示所有标签（瀑布图、月度序列），设置 `scales.x.ticks: { autoSkip: false, maxRotation: 45 }`——缺少标签会使条形无法识别。

**数字格式化**：负值显示为 `-$5M` 而非 `$-5M`——符号在货币符号之前。使用格式化函数：`(v) => (v < 0 ? '-' : '') + '$' + Math.abs(v) + 'M'`。

**图例** — 始终禁用 Chart.js 默认图例并构建自定义 HTML。默认使用圆点且无值；自定义 HTML 提供小方块、紧凑间距和百分比：

```js
plugins: { legend: { display: false } }
```

```html
<div style="display: flex; flex-wrap: wrap; gap: 16px; margin-bottom: 8px; font-size: 12px; color: var(--color-text-secondary);">
  <span style="display: flex; align-items: center; gap: 4px;"><span style="width: 10px; height: 10px; border-radius: 2px; background: #3266ad;"></span>Chrome 65%</span>
  <span style="display: flex; align-items: center; gap: 4px;"><span style="width: 10px; height: 10px; border-radius: 2px; background: #73726c;"></span>Safari 18%</span>
</div>
```

当数据是分类数据时（饼图、环形图、单系列条形图），在每个标签中包含值/百分比。将图例放在图表上方（`margin-bottom`）或下方（`margin-top`）——而非画布内部。

**仪表板布局** — 将汇总数字包装在指标卡片中（见 UI 片段）放在图表上方。图表 canvas 在下方无缝衔接，无需卡片包裹。使用 `sendPrompt()` 进行下钻：`sendPrompt('按地区分解第四季度')`。
