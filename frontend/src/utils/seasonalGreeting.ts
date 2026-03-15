const ZEN_GREETINGS = {
  立春: {
    morning: "春气始建，万物有灵。早。",
    noon: "东风既解，宜舒展，宜思索。",
    evening: "寒余雪尽，听春萌动。我在。",
    night: "更深露重，唯此灯火相伴。请说。",
  },
  雨水: {
    morning: "随风潜入夜。今日微雨，润心。",
    noon: "水木明瑟，万物生光。请讲。",
    evening: "细雨敲窗，最是挑灯夜读时。",
    night: "万籁俱寂，心言可诉。",
  },
  惊蛰: {
    morning: "一声雷动，万物皆醒。君亦早。",
    noon: "阳气初振，宜行，宜远。",
    evening: "虫鸣隐约，静听春雷后之寂。",
    night: "万籁俱寂，心言可诉。",
  },
  春分: {
    morning: "昼夜平分，万物清朗。早安。",
    noon: "春色正半，浮生半日，且坐。",
    evening: "阴阳相半，心平气和，请言。",
    night: "更深露重，唯此灯火相伴。请说。",
  },
  清明: {
    morning: "气清景明，万物皆洁。早。",
    noon: "惠风和畅，宜怀远，宜惜取。",
    evening: "暮色清寒，点茶叙旧，何如？",
    night: "万籁俱寂，心言可诉。",
  },
  谷雨: {
    morning: "雨生百谷，春事渐远。早。",
    noon: "润泽无声，万物皆有所得。",
    evening: "暮春迟迟，且留片刻茶香。",
    night: "万籁俱寂，心言可诉。",
  },
  立夏: {
    morning: "绿阴铺野，夏日始长。早。",
    noon: "万物至此皆长大。君有何图？",
    evening: "槐序初至，晚风微凉。请讲。",
    night: "万籁俱寂，心言可诉。",
  },
  小满: {
    morning: "物致于此，小得盈满。早。",
    noon: "江河水满，心念亦丰。且思。",
    evening: "满而不溢，止于至善。我在。",
    night: "更深露重，唯此灯火相伴。请说。",
  },
  芒种: {
    morning: "仲夏始忙，种有所得。早。",
    noon: "暑气渐升，且避喧嚣，静坐。",
    evening: "煮梅时节，灯下共叙。请言。",
    night: "万籁俱寂，心言可诉。",
  },
  夏至: {
    morning: "日之极长，万物繁茂。早。",
    noon: "蝉噪林静，心静自然凉。",
    evening: "昼长夜短，且惜此时清辉。",
    night: "万籁俱寂，心言可诉。",
  },
  小暑: {
    morning: "温风至，雷雨频。早安。",
    noon: "倏忽温风，宜消暑，宜定心。",
    evening: "萤火微明，且听风吟。请讲。",
    night: "更深露重，唯此灯火相伴。请说。",
  },
  大暑: {
    morning: "腐草为萤，土润溽暑。早。",
    noon: "极热之时，守一方清净地。",
    evening: "散发乘夕凉，开轩纳微风。",
    night: "万籁俱寂，心言可诉。",
  },
  立秋: {
    morning: "凉风至，白露生。今日早。",
    noon: "云高气爽，宜远眺，宜沉淀。",
    evening: "一叶落而知秋。君有何思？",
    night: "万籁俱寂，心言可诉。",
  },
  处暑: {
    morning: "暑气渐退，天地始肃。早。",
    noon: "谷盈仓满，离离暑云散。",
    evening: "凉蝉切切，静候君音。请讲。",
    night: "更深露重，唯此灯火相伴。请说。",
  },
  白露: {
    morning: "露从今夜白，月是故乡明。",
    noon: "水汽凝珠，心境渐凉。且坐。",
    evening: "蒹葭苍苍，白露为霜。我在。",
    night: "万籁俱寂，心言可诉。",
  },
  秋分: {
    morning: "昼夜同长，秋意平分。早。",
    noon: "金气清肃，宜收敛，宜深思。",
    evening: "漏长宵半，秋月正朗。请言。",
    night: "更深露重，唯此灯火相伴。请说。",
  },
  寒露: {
    morning: "露气寒冷，万物归静。早。",
    noon: "菊始黄华，秋意正浓。请讲。",
    evening: "寒意侵衣，围炉温酒。何如？",
    night: "万籁俱寂，心言可诉。",
  },
  霜降: {
    morning: "气肃而凝，露结为霜。早。",
    noon: "霜红满径，万物毕成。且看。",
    evening: "岁晚天寒，静听落叶声。",
    night: "万籁俱寂，心言可诉。",
  },
  立冬: {
    morning: "冬，终也，万物收藏。早。",
    noon: "水始冰，地始冻。静心处之。",
    evening: "闭塞成冬，唯灯火可亲。",
    night: "更深露重，唯此灯火相伴。请说。",
  },
  小雪: {
    morning: "雨凝为雪，天地渐简。早。",
    noon: "云寒雪降，万物潜藏。请讲。",
    evening: "煮雪烹茶，最是寂静时。",
    night: "万籁俱寂，心言可诉。",
  },
  大雪: {
    morning: "积雪盈尺，天地一白。早。",
    noon: "寒冬至极，守一颗赤子心。",
    evening: "窗含西岭雪，灯映案头书。",
    night: "万籁俱寂，心言可诉。",
  },
  冬至: {
    morning: "阴极之至，一阳始生。早。",
    noon: "昼短夜长，且行且珍惜。",
    evening: "围炉话旧，待春归。我在。",
    night: "更深露重，唯此灯火相伴。请说。",
  },
  小寒: {
    morning: "寒气凝于此。君请自暖。",
    noon: "雁北乡，鹊始巢。静候君言。",
    evening: "岁寒三友，独守此时清净。",
    night: "万籁俱寂，心言可诉。",
  },
  大寒: {
    morning: "坚冰深处，春意已萌。早。",
    noon: "岁终之时，总结过往。请讲。",
    evening: "腊尽春回，且待第一枝梅。",
    night: "更深露重，唯此灯火相伴。请说。",
  },
};

/**
 * 获取基于天时的禅意问候语
 * @returns {string} 禅意文案
 */
export function getZenGreetingBySeason(): string {
  const now = new Date();
  const month = now.getMonth() + 1;
  const day = now.getDate();
  const hour = now.getHours();

  // 1. 定义节气的大致日期（每月两节气，通常在 5-8 日和 20-23 日）
  // 数组格式：[节气名, 月份, 开始日期]
  const solarTerms: [string, number, number][] = [
    ["小寒", 1, 5],
    ["大寒", 1, 20],
    ["立春", 2, 4],
    ["雨水", 2, 19],
    ["惊蛰", 3, 5],
    ["春分", 3, 20],
    ["清明", 4, 4],
    ["谷雨", 4, 20],
    ["立夏", 5, 5],
    ["小满", 5, 21],
    ["芒种", 6, 5],
    ["夏至", 6, 21],
    ["小暑", 7, 7],
    ["大暑", 7, 22],
    ["立秋", 8, 7],
    ["处暑", 8, 23],
    ["白露", 9, 7],
    ["秋分", 9, 23],
    ["寒露", 10, 8],
    ["霜降", 10, 23],
    ["立冬", 11, 7],
    ["小雪", 11, 22],
    ["大雪", 12, 7],
    ["冬至", 12, 22],
  ];

  // 2. 找到当前日期所属的节气区间
  // 倒序查找：找到第一个“小于等于当前日期”的节气即为所属区间
  let currentTerm = "小寒"; // 默认初始
  for (let i = solarTerms.length - 1; i >= 0; i--) {
    const [name, m, d] = solarTerms[i];
    if (month > m || (month === m && day >= d)) {
      currentTerm = name;
      break;
    }
    // 处理跨年情况：如果还没到1月5日，则属于上一年的最后一个节气“冬至”
    if (i === 0 && (month < 1 || (month === 1 && day < 5))) {
      currentTerm = "冬至";
    }
  }

  // 3. 确定时辰
  let timeKey: "morning" | "noon" | "evening" | "night";
  if (hour >= 5 && hour < 11) timeKey = "morning";
  else if (hour >= 11 && hour < 16) timeKey = "noon";
  else if (hour >= 16 && hour < 20) timeKey = "evening";
  else timeKey = "night";

  // 4. 返回文案
  return ZEN_GREETINGS[currentTerm as keyof typeof ZEN_GREETINGS][timeKey];
}
