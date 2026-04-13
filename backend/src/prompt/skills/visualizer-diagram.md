---
name: visualizer-diagram
description: SVG图表绘制规范，包括流程图、结构图和示意图三种类型的布局规则、节点样式、箭头标记和坐标计算。在绘制任何SVG图表前加载。
---

## 图表类型
*“解释复利如何运作” / “进程调度器如何工作”*

**导致大多数图表失败的两条规则——在编写每个箭头和每个框之前检查这些：**
1.  **箭头交叉检查**：在编写任何 `<line>` 或 `<path>` 之前，根据已放置的每个框检查其坐标。如果该线条穿过任何矩形的内部（不仅仅是其源/目标），它将明显地穿过该框——请改用 L 形的 `<path>` 绕行。这也适用于穿过标签的箭头。
2.  **框宽来自最长标签**：在编写 `<rect>` 之前，找到其最长的子文本（通常是副标题）。`rect_width = max(title_chars × 8, subtitle_chars × 7) + 24`。一个 100px 宽的框最多容纳一个 10 字符的副标题。如果你的副标题是 "Files, APIs, streams"（20 字符），该框至少需要 164px——100px 会明显溢出。

**层级打包：** 在放置前计算总宽度。示例 — 4 个发布/订阅消费者框：
- **错误**：x=40,160,260,360 w=160 → 40-60px 重叠（4×160=640 > 480 可用空间）
- **正确**：x=50,200,350,500 w=130 gap=20 → 适应（4×130 + 3×20 = 580 ≤ 590 安全宽度；右边缘在 630 ≤ 640）
对树形结构自底向上工作：首先确定叶子层级的大小，父级宽度 ≥ 子级宽度之和。

**图表是最难的用例**——由于精确的坐标计算，它们的失败率最高。常见错误：viewBox 太小（内容被裁剪）、箭头穿过无关的框、标签放在箭头线上、文本超出 viewBox 边缘。对于说明图，还需注意：形状延伸到 viewBox 之外、重叠的标签遮挡了绘图、以及颜色选择不能直观地映射到所显示的物理属性。在最终确定前仔细检查坐标。

使用 `imagine_svg` 绘制图表。小部件会自动将 SVG 输出包装在卡片中。

**选择正确的图表类型。** 决策关乎 *意图*，而非主题。问：用户是想 *记录* 这个，还是 *理解* 它？

**参考图** — 用户想要一个可以指点的地图。精确性比感觉更重要。框、标签、箭头、包含关系。这些是你会在文档中找到的图表。
- **流程图** — 顺序步骤、决策分支、数据转换。适用于：审批工作流、请求生命周期、构建流水线、“当我点击提交时会发生什么”。触发短语：*“带我过一遍流程”*、*“步骤是什么”*、*“流程是什么”*。
- **结构图** — 事物内部的包含关系。适用于：文件系统（inode 中的块在分区中）、VPC/子网/实例、“细胞内部有什么”。触发短语：*“架构是什么样的”*、*“这是如何组织的”*、*“X 位于何处”*。

**直觉图** — 用户想要 *感受* 某物如何工作。目标不是正确的地图，而是正确的心智模型。这些图表看起来应该完全不像流程图。主题不需要有物理形态——它需要一个 *视觉隐喻*。
- **说明图** — 画出机制。物理事物画剖面图（热水器、发动机、肺）。抽象事物用空间隐喻：LLM 是一堆层，令牌随着注意力权重亮起；梯度下降是一个球滚过损失曲面；哈希表是一排桶，物品落入其中；TCP 是两个人传递带编号的信封。适用于：ML 概念（transformer、注意力、反向传播、嵌入）、物理直觉、计算机科学基础（指针、递归、调用栈）、任何通过 *看见* 而非 *阅读* 才能突破理解的事物。触发短语：*“X 实际上是如何工作的”*、*“解释 X”*、*“我不明白 X”*、*“给我一个关于 X 的直觉”*。

**根据动词路由，而非名词。** 同一主题，根据所问内容的不同，图表也不同：

| 用户说 | 类型 | 画什么 |
| :--- | :--- | :--- |
| “LLM 是如何工作的” | **说明图** | 令牌行、堆叠的层板、令牌之间发光的暖色注意力线程。如果可能，走向交互。 |
| “transformer 架构” | 结构图 | 带标签的框：嵌入、注意力头、FFN、层归一化。 |
| “注意力机制是如何工作的” | **说明图** | 一个查询令牌，连接到每个键的扇形线，线的不透明度 = 权重。 |
| “梯度下降是如何工作的” | **说明图** | 等高线曲面、一个球、一条步迹。带学习率滑块。 |
| “训练步骤是什么” | 流程图 | 前向 → 损失 → 反向 → 更新。框和箭头。 |
| “TCP 是如何工作的” | **说明图** | 两个端点，飞行中的带编号数据包，一个返回的 ACK。 |
| “TCP 握手序列” | 流程图 | SYN → SYN-ACK → ACK。三个框。 |
| “解释克雷布斯循环” / “事件循环是如何工作的” | **HTML 步进器** | 点击浏览各个阶段。绝不用环形图。 |
| “哈希映射是如何工作的” | **说明图** | 键通过漏斗落入 N 个桶中的一个。 |
| “画出数据库模式” / “给我展示 ER 图” | **mermaid.js** | `erDiagram` 语法。不是 SVG。 |

对于没有进一步限定的 *“X 是如何工作的”*，说明路线是默认选择。这是更富野心的选择——不要因为感觉更安全就退缩到流程图。Claude 很擅长画这些。

不要在一个图表中混合不同的图表家族。如果两者都需要，先画直觉版本（建立心智模型），然后画参考版本（填充精确标签），作为第二次工具调用，中间用文字隔开。

**对于复杂主题，使用多个 SVG 调用** — 将解释分解为一系列较小的图表，而不是一个密集的图表。每个 SVG 都会带着自己的动画和卡片流式传入，创建一个用户可以逐步跟随的视觉叙事。

**始终在图表之间添加文字** — 切勿在没有中间文本的情况下连续堆叠多个 SVG 调用。在每个 SVG 之间，写一个简短的段落（在你的正常回复文本中，工具调用之外），解释下一个图表显示什么以及它如何与上一个图表相关联。

**只承诺你能交付的** — 如果你的回复文本说“这里有三个图表”，你必须包含所有三个工具调用。切勿承诺后续图表却遗漏它。如果你只能放一个图表，请调整你的文本以匹配。一个完整的图表胜过三个承诺却只交付一个。

#### 流程图

用于顺序流程、因果关系、决策树。

**规划**：框的大小应能宽松容纳文本。在 14px 无衬线字体下，每个字符约 8px 宽——像“Load Balancer”（13 字符）这样的标签需要至少 140px 宽的矩形。不确定时，将框做得更宽，并在它们之间留出更多空间。拥挤的图表是最常见的失败模式。

**特殊字符更宽**：化学式（C₆H₁₂O₆）、数学符号（∑、∫、√）、通过 `<tspan>` 带 `dy`/`baseline-shift` 的上标/下标，以及 Unicode 符号，渲染宽度都比普通拉丁字符大。对于包含公式或特殊符号的标签，在你的宽度估计上增加 30-50% 的余量。不确定时，将框做得更宽——溢出比额外的内边距看起来更糟。

**间距**：框之间至少 60px，框内填充 24px，文本与边缘间距 12px。箭头头部与框边缘之间留出 10px 间隙。两行框（标题 + 副标题）至少需要 56px 高度，两行之间间距 22px。

**垂直文本放置**：框内的每个 `<text>` 需要 `dominant-baseline="central"`，y 设置为它所在插槽的 *中心*。没有它，SVG 将 y 视为基线，字形主体会比你的意图高出约 4px，下伸部会落到下一行。公式：对于在 (x, y, w, h) 矩形中居中的文本，使用 `<text x={x+w/2} y={y+h/2} text-anchor="middle" dominant-baseline="central">`。对于多行框内的一行，y 是 *该行* 的中心，而不是整个框的中心。

**布局**：优先选择单向流程（全自上而下或全自左而右）。保持图表简单——每个图表最多 4-5 个节点。小部件较窄（约 680px），因此复杂布局会出问题。

**当提示本身超出预算时**：如果用户列出了 6 个以上的组件（“给我画认证、产品、订单、支付、网关、队列”），不要一次性画出所有——你总会得到重叠的框和穿过文本的箭头。分解：(1) 一个精简的概览图，只有框和最多一两个显示主要流程的箭头——无扇出，无 N 对 N 网格；(2) 然后为每个有趣的子流程画一个图表（“这是下单时发生的情况”，“这是认证握手机制”），每个图表有 3-4 个节点并有呼吸空间。在绘制之前先数一下名词。用户要求完整性——通过几个图表给他们，而不是塞进一个。

**循环不应画成环。** 如果最后阶段反馈到第一阶段（克雷布斯循环、事件循环、GC 标记-清除、TCP 重传），你的直觉是将阶段围绕一个圆放置。不要。本规范中的每条间距规则都是笛卡尔坐标系的——没有针对“输入框在环上环绕阶段框”的碰撞检查。你会得到与它们所馈送的阶段重叠的卫星框、位于虚线圆上的标签，以及指向各处的切线箭头。环是装饰；循环是通过返回箭头来传达的。

在 `imagine_html` 中构建一个步进器。每个阶段一个面板，点或药丸形状显示位置（● ○ ○），下一步从最后一个阶段回到第一个——那就是循环。每个面板拥有其输入和产品：事件循环的待处理回调位于 *轮询* 面板 *内部*，而不是悬浮在环上的框旁边。没有任何东西碰撞，因为没有共享画布。仅当总共只有一个输入和一个输出且没有每个阶段的细节要显示时，才回退到线性 SVG（阶段排成一行，弯曲的 `<path>` 返回箭头）。

**线性流程中的反馈回路：** 不要画一个物理箭头横穿布局（它对抗流程方向并会裁剪边缘）。相反：
- 在循环点附近使用小的 `↻` 字形 + 文本：`<text>↻ returns to start</text>`
- 或者如果循环本身就是重点，将整个图重新构建为圆形

**箭头：** 从 A 到 B 的线绝不能穿过任何其他框或标签。如果直接路径穿过某物，用 L 形弯曲绕行：`<path d="M x1 y1 L x1 ymid L x2 ymid L x2 y2"/>`。将箭头标签放在清晰的空间中，而非中点上。

当所有节点具有相同内容类型时，保持它们高度一致（例如，所有单行框 = 44px，所有双行框 = 56px）。

**流程图组件** — 一致使用这些模式：

*单行节点*（44px 高）：仅标题。`c-blue` 类自动为浅色和深色模式设置填充、描边和文本颜色——无需 `<style>` 块。
```svg
<g class="node c-blue" onclick="sendPrompt('Tell me more about T-cells')">
  <rect x="100" y="20" width="180" height="44" rx="8" stroke-width="0.5"/>
  <text class="th" x="190" y="42" text-anchor="middle" dominant-baseline="central">T-cells</text>
</g>
```

*双行节点*（56px 高）：粗体标题 + 柔和副标题。
```svg
<g class="node c-blue" onclick="sendPrompt('Tell me more about dendritic cells')">
  <rect x="100" y="20" width="200" height="56" rx="8" stroke-width="0.5"/>
  <text class="th" x="200" y="38" text-anchor="middle" dominant-baseline="central">Dendritic cells</text>
  <text class="ts" x="200" y="56" text-anchor="middle" dominant-baseline="central">Detect foreign antigens</text>
</g>
```

*连接线*（无标签——含义从源 + 目标清晰可见）：
```svg
<line x1="200" y1="76" x2="200" y2="120" class="arr" marker-end="url(#arrow)"/>
```

*中性节点*（灰色，用于开始/结束/通用步骤）：使用 `class="box"` 获得自动主题化的填充/描边，并使用默认文本类。

默认使所有节点可点击——包裹在 `<g class="node" onclick="sendPrompt('...')">` 中。悬停效果是内置的。

#### 结构图

用于物理或逻辑包含关系重要的概念——事物内部的事物。

**何时使用**：解释取决于过程发生的 *位置*。示例：细胞如何工作（细胞器在细胞内）、文件系统如何工作（inode 中的块在分区中）、建筑暖通空调如何工作（风管在楼层内、楼层在建筑内）、CPU 缓存层次结构如何工作（L1 在内核内、L2 共享）。

**核心理念**：大圆角矩形是容器。内部的较小矩形是区域或子结构。文本标签描述每个区域中发生的事情。箭头显示区域之间或外部输入/输出之间的流动。

**容器规则**：
- 最外层容器：大圆角矩形，rx=20-24，最浅填充（50 梯度），0.5px 描边（600 梯度）。标签在内部左上角，14px 粗体。
- 内部区域：中等圆角矩形，rx=8-12，下一个色调填充（100-200 梯度）。如果区域在语义上与其父级不同，则使用不同的色阶。
- 每个容器内最小填充 20px——文本和内部区域不得接触容器边缘。
- 最多 2-3 层嵌套。在 680px 宽度下，更深的嵌套会变得难以阅读。

**布局**：
- 将内部区域并排放置在容器内，它们之间有 16px 以上的间隙。
- 外部输入（阳光、水、数据、请求）位于容器外部，箭头指向内。
- 外部输出位于外部，箭头指向外。
- 保持外部标签简短——一个词或一个短语。详细信息放在图表之间的文字中。

**区域内放置内容**：仅文本——区域名称（14px 粗体）和简短描述（12px）。不要将流程图式的框放入区域内。不要在里面画插图或图标。

**结构容器示例**（图书馆分馆，包含两个并排区域，一个内部带标签箭头，一个外部输入）。ViewBox 700x320，水平布局，颜色类处理浅色和深色模式——无需 `<style>` 块：
```svg
<defs>
  <marker id="arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
    <path d="M2 1L8 5L2 9" fill="none" stroke="context-stroke" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
  </marker>
</defs>
<!-- Outer container -->
<g class="c-green">
  <rect x="120" y="30" width="560" height="260" rx="20" stroke-width="0.5"/>
  <text class="th" x="400" y="62" text-anchor="middle">Library branch</text>
  <text class="ts" x="400" y="80" text-anchor="middle">Main floor</text>
</g>
<!-- Inner: Circulation desk -->
<g class="c-teal">
  <rect x="150" y="100" width="220" height="160" rx="12" stroke-width="0.5"/>
  <text class="th" x="260" y="130" text-anchor="middle">Circulation desk</text>
  <text class="ts" x="260" y="148" text-anchor="middle">Checkouts, returns</text>
</g>
<!-- Inner: Reading room -->
<g class="c-amber">
  <rect x="450" y="100" width="210" height="160" rx="12" stroke-width="0.5"/>
  <text class="th" x="555" y="130" text-anchor="middle">Reading room</text>
  <text class="ts" x="555" y="148" text-anchor="middle">Seating, reference</text>
</g>
<!-- Arrow between inner boxes with label -->
<text class="ts" x="410" y="175" text-anchor="middle">Books</text>
<line x1="370" y1="185" x2="448" y2="185" class="arr" marker-end="url(#arrow)"/>
<!-- External input: New acq. — text vertically aligned with arrow -->
<text class="ts" x="40" y="185" text-anchor="middle">New acq.</text>
<line x1="75" y1="185" x2="118" y2="185" class="arr" marker-end="url(#arrow)"/>
```

**结构图中的颜色**：嵌套区域需要不同的色阶——`c-{ramp}` 类解析为固定的填充/描边梯度，因此父级和子级上使用相同的类会产生相同的填充并使层次结构扁平化。为内部结构选择一个 *相关* 的色阶（例如图书馆外壳用绿色，其内部的流通台用蓝绿色），并为功能不同的区域选择一个 *对比* 色阶（例如阅览室用琥珀色）。这使图表一目了然——你可以一眼看出哪些部分是相关的。

**数据库模式 / ER 图 — 使用 mermaid.js，而不是 SVG。** 模式表是表头加 N 个字段行加类型列加鱼尾纹连接器。这是一个文本布局问题，手动用 SVG 放置每次都会以相同方式失败。mermaid.js `erDiagram` 免费提供布局、基数和连接器路由。仅限 ER 图；其他所有内容仍用 SVG。

```
erDiagram
  USERS ||--o{ POSTS : writes
  POSTS ||--o{ COMMENTS : has
  USERS {
    uuid id PK
    string email
    timestamp created_at
  }
  POSTS {
    uuid id PK
    uuid user_id FK
    string title
  }
```

使用 `imagine_html` 绘制 ER 图。在 `<script type="module">` 中导入并初始化。主机 CSS 会重新设计 mermaid 输出样式以匹配设计系统——保持初始化块完全如所示（`fontFamily` + `fontSize` 用于布局测量；偏离会导致文本裁剪）。渲染后，将尖角的实体 `<path>` 元素替换为圆角的 `<rect rx="8">` 以匹配设计系统，并移除属性行的边框（仅外部容器和标题行保留可见边框——交替的填充色分隔各行）：
```html
<style>
#erd svg.erDiagram .divider path { stroke-opacity: 0.5; }
#erd svg.erDiagram .row-rect-odd path,
#erd svg.erDiagram .row-rect-odd rect,
#erd svg.erDiagram .row-rect-even path,
#erd svg.erDiagram .row-rect-even rect { stroke: none !important; }
</style>
<div id="erd"></div>
<script type="module">
import mermaid from 'https://esm.sh/mermaid@11/dist/mermaid.esm.min.mjs';
const dark = matchMedia('(prefers-color-scheme: dark)').matches;
await document.fonts.ready;
mermaid.initialize({
  startOnLoad: false,
  theme: 'base',
  fontFamily: '"Anthropic Sans", sans-serif',
  themeVariables: {
    darkMode: dark,
    fontSize: '13px',
    fontFamily: '"Anthropic Sans", sans-serif',
    lineColor: dark ? '#9c9a92' : '#73726c',
    textColor: dark ? '#c2c0b6' : '#3d3d3a',
  },
});
const { svg } = await mermaid.render('erd-svg', `erDiagram
  USERS ||--o{ POSTS : writes
  POSTS ||--o{ COMMENTS : has`);
document.getElementById('erd').innerHTML = svg;

// Round only the outermost entity box corners (not internal row stripes)
document.querySelectorAll('#erd svg.erDiagram .node').forEach(node => {
  const firstPath = node.querySelector('path[d]');
  if (!firstPath) return;
  const d = firstPath.getAttribute('d');
  const nums = d.match(/-?[\d.]+/g)?.map(Number);
  if (!nums || nums.length < 8) return;
  const xs = [nums[0], nums[2], nums[4], nums[6]];
  const ys = [nums[1], nums[3], nums[5], nums[7]];
  const x = Math.min(...xs), y = Math.min(...ys);
  const w = Math.max(...xs) - x, h = Math.max(...ys) - y;
  const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
  rect.setAttribute('x', x); rect.setAttribute('y', y);
  rect.setAttribute('width', w); rect.setAttribute('height', h);
  rect.setAttribute('rx', '8');
  for (const a of ['fill', 'stroke', 'stroke-width', 'class', 'style']) {
    if (firstPath.hasAttribute(a)) rect.setAttribute(a, firstPath.getAttribute(a));
  }
  firstPath.replaceWith(rect);
});

// Strip borders from attribute rows (mermaid v11: .row-rect-odd / .row-rect-even)
document.querySelectorAll('#erd svg.erDiagram .row-rect-odd path, #erd svg.erDiagram .row-rect-even path').forEach(p => {
  p.setAttribute('stroke', 'none');
});
</script>
```

对 `classDiagram` 同样有效——替换图表源；初始化保持不变。

#### 说明图

用于建立 *直觉*。主题可能是物理的（发动机、肺）或完全抽象的（注意力、递归、梯度下降）——重要的是空间绘图比带标签的框更能传达机制。这些图表会让人感叹：“哦，*原来* 它是在做这个。”

**两种风格，相同的规则：**
- **物理主题** 被绘制成自身的简化版本。剖面图、切面图、示意图。热水器是一个带下方燃烧器的水箱。肺是空腔中的分支树。你在画 *那个东西*，风格化了的。
- **抽象主题** 被绘制成 *空间隐喻*。你在为没有形态的东西发明一个形状——但这个形状应该使机制显而易见。Transformer 是一堆水平的平板，一条明亮的注意力线索将各层的令牌连接起来。哈希函数是一个漏斗，将物品分散到一排桶中。调用栈字面上就是一个生长和收缩的栈帧。嵌入是空间中聚集的点。隐喻 *就是* 解释。

这是最富野心的图表类型，也是 Claude 最擅长的。放手去做。用颜色表示强度（高注意力权重发出琥珀色光芒，低权重保持灰色）。用重复表示规模（许多小圆圈 = 许多参数）。

**优先选择交互式而非静态。** 静态剖面图是一个好的答案；一个你可以 *操作* 的剖面图是一个极好的答案。决策规则：如果现实世界系统有一个控件，就给图表那个控件。热水器有恒温器——所以给用户一个滑块来移动热/冷边界，一个切换开关来点燃燃烧器并动画化对流。LLM 有输入令牌——让用户点击一个，观察注意力权重重新扇形展开。缓存有命中率——让他们拖动它，观察延迟变化。首先考虑使用带有内联 SVG 的 `imagine_html`；只有在确实没有什么可调整时才回退到静态 `imagine_svg`。

**何时不使用**：用户要求的是 *参考*，而不是 *直觉*。“Transformer 的组件是什么” 想要带标签的框——那是结构图。“带我过一遍我们的 CI 流水线” 想要顺序步骤——那是流程图。当隐喻会武断而非启发时也应跳过：把“云”画成云形或把“微服务”画成小房子并不能教会它们如何工作。如果绘图没有使 *机制* 更清晰，就不要画。

**保真度上限**：这些是示意图，不是插画。每个形状都应一目了然。如果一个 `<path>` 需要超过 ~6 个线段来绘制，就简化它。水箱是一个圆角矩形，不是水箱的贝塞尔肖像。火焰是三个三角形，不是一团火。可识别的轮廓每次都胜过精确的轮廓——如果你发现自己在仔细描摹一个轮廓，说明你用力过猛了。

**核心原则**：绘制机制，而不是一个 *关于* 机制的图表。空间布局承载含义；标签是注释。一个好的说明图在移除标签后仍然有效。

**与流程图/结构图规则的不同之处**：

- **形状是自由形式的。** 使用 `<path>`、`<ellipse>`、`<circle>`、`<polygon>` 和曲线来表示真实形态。水箱是一个底部圆角的高矩形。心脏瓣膜是一对弯曲的路径。电路走线是一条细折线。你不受限于圆角矩形。
- **布局遵循主题的几何形状**，而非网格。如果事物是高而窄的（热水器、温度计），图表就是高而窄的。如果是宽而平的（PCB、地质剖面图），图表就是宽的。让主题在 680px viewBox 宽度内决定比例。
- **颜色编码强度**，而非类别。对于物理主题：暖色阶（琥珀色、珊瑚色、红色）= 热量/能量/压力，冷色阶（蓝色、蓝绿色）= 寒冷/平静，灰色 = 惰性结构。对于抽象主题：暖色 = 活跃/高权重/受关注，冷色或灰色 = 休眠/低权重/被忽略。用户应能瞥一眼图表，无需阅读任何标签就能看到 *动作发生在哪里*。
- **鼓励形状的层叠和重叠。** 与流程图中框绝不得重叠不同，说明图可以层叠形状以求深度——管道进入水箱、注意力线穿过层、绝缘层包裹腔室。有意识地使用 z 顺序（源代码中越晚 = 越靠上）。
- **文本是例外——绝不让描边穿过它。** 重叠许可仅适用于形状。每个标签在其基线和上升部之间以及最近的描边之间需要 8px 的净空。不要用背景矩形解决这个问题——通过 *将文本放在别处* 来解决。标签放在安静的区域：绘图上方、下方、边距中带引导线，或两扇形线之间的间隙。如果没有安静区域，绘图太密集了——移除一些东西或拆分成两个图表。
- **允许使用小的基于形状的指示器**，当它们传达物理状态时。火焰用三角形。气泡或粒子用圆圈。蒸汽或热辐射用波浪线。振动用平行线。这些不是装饰——它们告诉用户物理上正在发生什么。保持简单：基本 SVG 图元，而非详细插画。
- **每张图允许一个渐变** — 这是全局无渐变规则的唯一例外 — 并且仅用于显示一个区域上 *连续* 的物理属性（水箱中的温度分层、管道中的压降、溶液中的浓度）。它必须是一个单独的 `<linearGradient>`，在恰好两个来自同一色阶的梯度之间。无径向渐变、无多梯度淡出、无美学性渐变。如果两个堆叠的纯色填充矩形能达到同样效果，就用那个。
- **交互式 HTML 版本允许动画。** 使用 CSS `@keyframes`，仅动画化 `transform` 和 `opacity`。保持循环在约 2 秒以下，并将每个动画包裹在 `@media (prefers-reduced-motion: no-preference)` 中，使其默认为可选择退出。动画应显示系统如何 *表现*——对流、旋转、流动——而非为动而动。无物理引擎或重型库。

所有核心规则仍然适用（viewBox 680px，深色模式强制，14/12px 文本，预构建类，箭头标记，可点击节点）。

**标签放置**：
- 尽可能将标签放在绘制对象 *外部*，用细引导线（0.5px 虚线，`var(--t)` 描边）指向相关部分。这使插图保持整洁。
- 对于大的内部区域（如水箱中的温度区），如果内部有充足的清晰空间——距任何边缘至少 20px——标签可以放在内部。
- 外部标签位于边距区域或对象上方/下方。**为标签选择一侧并将它们全部放在那里**——在 680px 宽度下，你没有空间同时容纳绘图 *和* 两侧的标签列。在标签侧预留至少 140px 的水平边距。左侧的标签容易裁剪：`text-anchor="end"` 从 x 向左延伸，对于多行标注很容易不知不觉超出 x=0。除非主题几何形状强制，否则默认使用右侧标签和 `text-anchor="start"`。对标注使用 `class="ts"`（12px），对主要组件名称使用 `class="th"`（14px 中等）。

**构图方法**：
1.  从主对象的轮廓开始——最大的形状，在 viewBox 中居中。
2.  添加内部结构：腔室、管道、膜、机械部件。
3.  添加外部连接：进出管道、显示流向的箭头、输入和输出的标签。
4.  最后添加状态指示器：显示温度/压力/浓度的颜色填充，显示运动或能量的小型动画元素。
5.  在对象周围留出充足的空白用于标签——不要将注释挤在 viewBox 边缘。

**静态 vs 交互式**：静态剖面图和切面图最好作为纯 `imagine_svg`。如果图表受益于控件——一个改变温度区域的滑块、切换操作状态的按钮、实时读数——使用带有内联 SVG 用于绘图和周围 HTML 控件的 `imagine_html`。

**说明图示例** — 交互式热水器剖面图，具有生动的物理写实色彩、动画对流和控件。使用带有内联 SVG 的 `imagine_html`：恒温器滑块移动热/冷渐变边界，加热开关动画化火焰的开/关，并将对流过渡到暂停。viewBox 为 680x560；水箱占据 x=180..440，为右侧标签留出 140px 以上的边距。平滑的对流路径使用 `stroke-dasharray:5 5`，约 1.6 秒，营造温和的流动感。加热时，热水区的暖光叠加层会微妙脉动。火焰形状使用暖色渐变填充和干净的不透明度过渡。标签沿右侧边距排列，带引导线。
```html
<style>
  @keyframes conv { to { stroke-dashoffset: -20; } }
  @keyframes flicker { 0%,100%{opacity:1} 50%{opacity:.82} }
  @keyframes glow { 0%,100%{opacity:.3} 50%{opacity:.6} }
  .conv { stroke-dasharray:5 5; animation: conv var(--dur,1.6s) linear infinite; transition: opacity .5s; }
  .conv.off { opacity:0; animation-play-state:paused; }
  #flames path { transition: opacity .5s; }
  #flames.off path { opacity:0; animation:none; }
  #flames path:nth-child(odd)  { animation: flicker .6s ease-in-out infinite; }
  #flames path:nth-child(even) { animation: flicker .8s ease-in-out infinite .15s; }
  #warm-glow { animation: glow 3s ease-in-out infinite; transition: opacity .5s; }
  #warm-glow.off { opacity:0; animation:none; }
  .toggle-track { position:relative;width:32px;height:18px;background:var(--color-border-secondary);border-radius:9px;transition:background .2s;display:inline-block; }
  .toggle-track:has(input:checked) { background:var(--color-text-info); }
  #heat-toggle:checked + span { transform:translateX(14px); }
</style>
<svg width="100%" viewBox="0 0 680 560">
  <defs>
    <marker id="arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M2 1L8 5L2 9" fill="none" stroke="context-stroke" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></marker>
    <linearGradient id="tg" x1="0" y1="0" x2="0" y2="1">
      <stop id="gh" offset="40%" stop-color="#E8593C" stop-opacity="0.45"/>
      <stop id="gc" offset="40%" stop-color="#3B8BD4" stop-opacity="0.4"/>
    </linearGradient>
    <linearGradient id="fg1" x1="0" y1="1" x2="0" y2="0"><stop offset="0%" stop-color="#E85D24"/><stop offset="60%" stop-color="#F2A623"/><stop offset="100%" stop-color="#FCDE5A"/></linearGradient>
    <linearGradient id="fg2" x1="0" y1="1" x2="0" y2="0"><stop offset="0%" stop-color="#D14520"/><stop offset="50%" stop-color="#EF8B2C"/><stop offset="100%" stop-color="#F9CB42"/></linearGradient>
    <linearGradient id="pipe-h" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="#D05538" stop-opacity=".25"/><stop offset="100%" stop-color="#D05538" stop-opacity=".08"/></linearGradient>
    <linearGradient id="pipe-c" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="#3B8BD4" stop-opacity=".25"/><stop offset="100%" stop-color="#3B8BD4" stop-opacity=".08"/></linearGradient>
    <clipPath id="tc"><rect x="180" y="55" width="260" height="390" rx="14"/></clipPath>
  </defs>
  <!-- Tank fill -->
  <g clip-path="url(#tc)"><rect x="180" y="55" width="260" height="390" fill="url(#tg)"/></g>
  <!-- Warm glow overlay (pulses when heating) -->
  <g clip-path="url(#tc)"><rect id="warm-glow" x="180" y="55" width="260" height="160" fill="#E8593C" opacity=".3"/></g>
  <!-- Tank shell (double stroke for solidity) -->
  <rect x="180" y="55" width="260" height="390" rx="14" fill="none" stroke="var(--t)" stroke-width="2.5" opacity=".25"/>
  <rect x="180" y="55" width="260" height="390" rx="14" fill="none" stroke="var(--t)" stroke-width="1"/>
  <!-- Hot pipe out (top right) -->
  <rect x="370" y="14" width="16" height="50" rx="4" fill="url(#pipe-h)"/>
  <path d="M378 14V55" stroke="var(--t)" stroke-width="3" stroke-linecap="round" fill="none"/>
  <!-- Cold pipe in + dip tube (top left) -->
  <rect x="234" y="14" width="16" height="50" rx="4" fill="url(#pipe-c)"/>
  <path d="M242 14V55" stroke="var(--t)" stroke-width="3" stroke-linecap="round" fill="none"/>
  <path d="M242 55V395" stroke="var(--t)" stroke-width="2.5" stroke-linecap="round" fill="none" opacity=".5"/>
  <!-- Convection currents (curved paths at different speeds) -->
  <path class="conv" style="--dur:1.6s" fill="none" stroke="#D05538" stroke-width="1" opacity=".5" d="M350 380C355 320,365 240,358 140Q355 110,340 100"/>
  <path class="conv" style="--dur:2.1s" fill="none" stroke="#C04828" stroke-width=".8" opacity=".35" d="M300 390C308 340,320 260,315 170Q312 130,298 115"/>
  <path class="conv" style="--dur:2.6s" fill="none" stroke="#B05535" stroke-width=".7" opacity=".3" d="M380 370C382 310,388 230,382 150Q378 120,365 110"/>
  <!-- Burner bar -->
  <rect x="188" y="454" width="244" height="5" rx="2" fill="var(--t)" opacity=".6"/>
  <rect x="220" y="462" width="180" height="6" rx="3" fill="var(--t)" opacity=".3"/>
  <!-- Flames (gradient-filled organic shapes) -->
  <g id="flames">
    <path d="M240,454Q248,430 252,438Q256,424 260,454Z" fill="url(#fg1)"/>
    <path d="M278,454Q285,426 290,434Q295,418 300,454Z" fill="url(#fg2)"/>
    <path d="M320,454Q328,428 333,436Q338,420 342,454Z" fill="url(#fg1)"/>
    <path d="M360,454Q367,430 371,438Q375,422 380,454Z" fill="url(#fg2)"/>
    <path d="M398,454Q404,434 408,440Q412,428 416,454Z" fill="url(#fg1)"/>
  </g>
  <!-- Labels (right margin) -->
  <g class="node" onclick="sendPrompt('How does hot water exit the tank?')">
    <line class="leader" x1="386" y1="34" x2="468" y2="70"/><circle cx="386" cy="34" r="2" fill="var(--t)"/>
    <text class="ts" x="474" y="74">Hot water outlet</text></g>
  <g class="node" onclick="sendPrompt('How does the cold water inlet work?')">
    <line class="leader" x1="250" y1="34" x2="468" y2="140"/><circle cx="250" cy="34" r="2" fill="var(--t)"/>
    <text class="ts" x="474" y="144">Cold water inlet</text></g>
  <g class="node" onclick="sendPrompt('What does the dip tube do?')">
    <line class="leader" x1="250" y1="260" x2="468" y2="220"/><circle cx="250" cy="260" r="2" fill="var(--t)"/>
    <text class="ts" x="474" y="224">Dip tube</text></g>
  <g class="node" onclick="sendPrompt('What does the thermostat control?')">
    <line class="leader" x1="440" y1="250" x2="468" y2="300"/><circle cx="440" cy="250" r="2" fill="var(--t)"/>
    <text class="ts" x="474" y="304">Thermostat</text></g>
  <g class="node" onclick="sendPrompt('What material is the tank made of?')">
    <line class="leader" x1="440" y1="380" x2="468" y2="380"/><circle cx="440" cy="380" r="2" fill="var(--t)"/>
    <text class="ts" x="474" y="384">Tank wall</text></g>
  <g class="node" onclick="sendPrompt('How does the gas burner heat water?')">
    <line class="leader" x1="432" y1="454" x2="468" y2="454"/><circle cx="432" cy="454" r="2" fill="var(--t)"/>
    <text class="ts" x="474" y="458">Heating element</text></g>
</svg>
<div style="display:flex;align-items:center;gap:16px;margin:12px 0 0;font-size:13px;color:var(--color-text-secondary)">
  <label style="display:flex;align-items:center;gap:6px;cursor:pointer;user-select:none">
    <span class="toggle-track">
      <input type="checkbox" id="heat-toggle" checked onchange="toggleHeat(this.checked)" style="position:absolute;opacity:0;width:100%;height:100%;cursor:pointer;margin:0">
      <span style="position:absolute;top:2px;left:2px;width:14px;height:14px;background:#fff;border-radius:50%;transition:transform .2s;pointer-events:none"></span>
    </span>
    Heating
  </label>
  <span>Thermostat</span>
  <input type="range" id="temp-slider" min="10" max="90" value="40" style="flex:1" oninput="setTemp(this.value)">
  <span id="temp-label" style="min-width:36px;text-align:right">40%</span>
</div>
<script>
function setTemp(v) {
  document.getElementById('gh').setAttribute('offset', v+'%');
  document.getElementById('gc').setAttribute('offset', v+'%');
  document.getElementById('temp-label').textContent = v+'%';
}
function toggleHeat(on) {
  document.getElementById('flames').classList.toggle('off', !on);
  document.getElementById('warm-glow').classList.toggle('off', !on);
  document.querySelectorAll('.conv').forEach(p => p.classList.toggle('off', !on));
}
</script>
```

**说明图示例 — 抽象主题**（Transformer 中的注意力）。相同的规则，无物理对象。底部一行令牌，一个查询令牌高亮显示，按权重缩放的线扇形连接到每个其他令牌。说明文字放在扇形下方——远离任何描边——不在其内部。
```svg
<rect class="c-purple" x="60" y="40"  width="560" height="26" rx="6" stroke-width="0.5"/>
<rect class="c-purple" x="60" y="80"  width="560" height="26" rx="6" stroke-width="0.5"/>
<rect class="c-purple" x="60" y="120" width="560" height="26" rx="6" stroke-width="0.5"/>
<text class="ts" x="72" y="57" >Layer 3</text>
<text class="ts" x="72" y="97" >Layer 2</text>
<text class="ts" x="72" y="137">Layer 1</text>

<line stroke="#EF9F27" stroke-linecap="round" x1="340" y1="230" x2="116" y2="146" stroke-width="1"   opacity="0.25"/>
<line stroke="#EF9F27" stroke-linecap="round" x1="340" y1="230" x2="228" y2="146" stroke-width="1.5" opacity="0.4"/>
<line stroke="#EF9F27" stroke-linecap="round" x1="340" y1="230" x2="340" y2="146" stroke-width="4"   opacity="1.0"/>
<line stroke="#EF9F27" stroke-linecap="round" x1="340" y1="230" x2="452" y2="146" stroke-width="2.5" opacity="0.7"/>
<line stroke="#EF9F27" stroke-linecap="round" x1="340" y1="230" x2="564" y2="146" stroke-width="1"   opacity="0.2"/>

<g class="node" onclick="sendPrompt('What do the attention weights mean?')">
  <rect class="c-gray"  x="80"  y="230" width="72" height="36" rx="6" stroke-width="0.5"/>
  <rect class="c-gray"  x="192" y="230" width="72" height="36" rx="6" stroke-width="0.5"/>
  <rect class="c-amber" x="304" y="230" width="72" height="36" rx="6" stroke-width="1"/>
  <rect class="c-gray"  x="416" y="230" width="72" height="36" rx="6" stroke-width="0.5"/>
  <rect class="c-gray"  x="528" y="230" width="72" height="36" rx="6" stroke-width="0.5"/>
  <text class="ts" x="116" y="252" text-anchor="middle">the</text>
  <text class="ts" x="228" y="252" text-anchor="middle">cat</text>
  <text class="th" x="340" y="252" text-anchor="middle">sat</text>
  <text class="ts" x="452" y="252" text-anchor="middle">on</text>
  <text class="ts" x="564" y="252" text-anchor="middle">the</text>
</g>

<text class="ts" x="340" y="300" text-anchor="middle">Line thickness = attention weight from "sat" to each token</text>
```

注意这里 *没有* 的东西：没有标有“多头注意力”的框，没有标有“Q/K/V”的箭头。那些属于结构图。这个图是关于注意力的 *感觉*——一个令牌以不同强度看向每个其他令牌。

这些是起点，而非上限。对于热水器：添加恒温器滑块，动画化对流，切换加热 vs 待机。对于注意力图：让用户点击任何令牌成为查询，在各层之间滑动，动画化权重稳定过程。目标始终是 *展示* 事物如何工作，而不仅仅是 *标记* 它。
