# 牌型分析可视化模块工作报告

## 一、调研范围

本次遍历范围集中在实时牌型分析链路和前端可视化入口：

- 后端分析输出：`src/analysis/`，重点为 `result.rs`、`mod.rs`、`improves.rs`、`search.rs`、`risk/`。
- IPC 与前端状态：`frontend/src/types.ts`、`frontend/src/hooks/useTauriBridge.ts`、`frontend/src/stores/analysisStore.ts`。
- 实时可视化磁贴：`frontend/src/tiles/`，重点为 `SelfHandTile.tsx`、`RecommendationsTile.tsx`、`RiskChartTile.tsx`、`OpponentsTile.tsx`、`registry.tsx`、`defaults.ts`。
- 牌面渲染基础设施：`frontend/src/components/Mahgen.tsx`、`frontend/src/lib/tileIdx.ts`。

结论：当前后端已经产出较完整的牌型分析数据，前端实时可视化只消费了其中一部分。新增可视化模块可以沿用现有的 `TileFrame + useAnalysisStore + Mahgen / 表格 / 轻量条形图` 模式，不必改变后端分析事件管道。

## 二、现有分析数据结构

后端顶层入口是 `analysis::analyze(info)`。它按当前手牌张数分流：

- 13 张等待状态：填充 `hand13`，用于展示向听、听牌、改良、和牌率、点数期望等。
- 14 张打牌状态：填充 `hand14`，枚举切牌候选；每个候选内嵌一次切牌后的 `Hand13Result`。
- 两种状态都会计算对手风险、综合风险、最佳进攻切牌、最佳防守切牌。

关键字段来自 `AnalysisResult`：

- `shanten`：当前向听数。
- `state`：`wait13` 或 `discard14`。
- `hand13.waits`：有效进张列表，每项包含牌、剩余枚数、和牌率。
- `hand13.next_shanten_waits_count`：摸到有效牌之后，再做最佳切牌可得到的下一层听牌枚数。
- `hand13.mixed_waits_score`：速度评分，综合当前进张和下一层进张。
- `hand13.improves`：非直接进张但能扩大牌型的摸牌改良。
- `hand13.avg_agari_rate`、`dama_point`、`riichi_point`、`mixed_round_point`：和牌率与点数期望。
- `hand14.maintain`：不退向的切牌候选，已按速度评分、进张枚数、和牌率排序。
- `hand14.backwards`：退向候选，当前保留但未展示。
- `opponents[].risk` 与 `mixed_risk`：34 种牌的逐牌放铳风险。
- `best_attack_discard`、`best_defence_discard`：进攻与防守倾向的推荐切牌。

这说明当前可视化模块的扩展瓶颈主要在前端展示，而不是后端数据缺失。

## 三、现有可视化模块内容

### 1. 自家手牌

`SelfHandTile` 读取 `useGameStore` 中的 `MahgenView`，取 `view.players[ourSeat].hand` 后交给 `<Mahgen>` 渲染。该模块不直接消费分析结果，但它是牌型分析视图的基础参照。

实现特征：

- 使用 `TileFrame` 统一外壳。
- 使用 `<Mahgen seq={hand} kind="hand" />` 显示手牌。
- 后端已经提前将手牌转换成 mahgen DSL，前端只负责渲染。

### 2. 推荐切牌

`RecommendationsTile` 读取 `useAnalysisStore.result`，取 `result.hand14.maintain.slice(0, 3)` 展示前三个不退向切牌候选。

当前展示内容：

- 切牌牌面。
- 切牌后的 `mixed_round_point`，标记为 EV。
- 切牌后的 `avg_agari_rate`。
- 切牌后的 `waits_total`。
- 右上角展示当前向听数。

实现特征：

- 单张切牌通过 `mjaiToMahgen([c.discard])` 转为 mahgen DSL。
- 候选排序由后端 `analyze_14` 完成，前端只取前三。
- UI 是列表卡片，不使用图表库。

当前不足：

- 未展示 `best_attack_discard` 和 `best_defence_discard` 的对比。
- 未展示候选切牌后的具体进张牌。
- 未展示退向候选的代价，用户难以理解“为什么某些牌不推荐”。

### 3. 综合放铳风险

`RiskChartTile` 读取 `result.mixed_risk`，按 34 种牌逐行展示风险条。

当前展示内容：

- 牌种标签。
- 风险百分比。
- 低、中、高三段颜色：小于 10 为绿色，10 到 20 为黄色，20 以上为红色。

实现特征：

- 使用 `TILE_LABELS_34` 保持 34 维风险数组与牌名一致。
- 使用 CSS 宽度条模拟横向条形图，没有引入 Recharts。
- 数据来自混合风险，已按对手听牌率做门控和组合。

当前不足：

- 只展示综合风险，不展示每个对手的风险差异。
- 34 行列表阅读成本高，缺少按手牌可切牌过滤的视图。
- 没有直接标注最佳防守切牌。

### 4. 对手概览

`OpponentsTile` 读取 `result.opponents`，用表格展示每个对手的听牌率、是否立直、最大风险。

当前展示内容：

- 相对座位。
- 听牌率。
- 立直标记。
- 该对手 34 维风险中的最大值。

实现特征：

- 使用项目已有的 `Table` 组件。
- 座位标签按 3 人/4 人场分别处理。
- 该模块是风险模型的摘要入口。

当前不足：

- 未展示某个对手最危险的具体牌。
- 未展示各对手对同一候选切牌的风险贡献。

### 5. 历史图表

历史页已有 Recharts 实现：

- `RankPieChart`：名次分布饼图。
- `CumulativePtChart`：累计 PT 折线图。

虽然历史页不属于实时牌型分析，但它说明项目已经接受 Recharts 作为图表依赖。若新增实时分析图表需要折线、饼图、雷达图或堆叠条形图，可以复用相同依赖和主题变量。

## 四、现有实现方式总结

### 数据流

当前实时分析数据流如下：

1. 后端 `analysis::runner` 在 tracker 处理事件后生成 `AnalysisResult`。
2. 后端通过 Tauri 事件发出 `analysis-result`。
3. 前端 `useTauriBridge` 监听该事件，并写入 `useAnalysisStore`。
4. 各个磁贴组件通过 Zustand selector 读取 `result` 的局部字段。
5. 牌面使用 `<Mahgen>`，数值使用表格、列表或 CSS 条形条展示。

### 模块注册方式

新增实时可视化模块需遵循当前磁贴体系：

1. 在 `frontend/src/tiles/defaults.ts` 增加新的 `TileId`。
2. 将新 id 加入 `ALL_TILES`、`TILE_TITLES`、4 人和 3 人默认布局。
3. 在 `frontend/src/tiles/registry.tsx` import 新组件，并在 `renderTile` 中补一个 case。
4. 在 `frontend/src/components/AddTileMenu.tsx` 增加标题 i18n key 映射。
5. 在 `frontend/src/i18n/resources/*.json` 添加标题与空态文案。
6. 新组件内部使用 `TileFrame`，通过 `useAnalysisStore` 读取分析数据。

### 牌面渲染方式

项目使用 `<mah-gen>` Web Component，前端封装为 `Mahgen`：

- 后端 `MahgenView` 已可直接提供完整手牌、河牌、副露 DSL。
- 分析结果中仍是 mjai 字符串时，前端使用 `mjaiToMahgen()` 转换。
- 单张推荐、进张、改良摸牌都可以复用这一方式。

### 图表方式

实时分析页现有风险模块使用 CSS 条形条，历史页使用 Recharts。建议新增模块按复杂度选择：

- 牌面、短列表、候选对比：继续使用 `TileFrame + Mahgen + CSS`。
- 多维趋势、分布、堆叠对比：复用 Recharts，保持 `ResponsiveContainer` 和 CSS 变量配色。

## 五、可新增的可视化模块建议

### 1. 进张与和牌率模块

目标：补齐 13 张等待状态下的核心信息，让用户看到“现在摸什么有用”。

数据来源：

- `result.hand13.waits`
- `result.hand13.waits_total`
- `result.hand13.avg_agari_rate`
- `result.hand13.is_furiten`
- `result.hand13.furiten_rate`

展示方式：

- 顶部显示向听、总进张枚数、平均和牌率。
- 下方按 `left` 或 `agari_rate` 排序，逐行显示牌面、剩余枚数、单牌和牌率。
- 听牌振听时增加醒目标记。

实现方式：

- 新建 `WaitsTile.tsx`。
- 组件读取 `const hand13 = useAnalysisStore((s) => s.result?.hand13 ?? null)`。
- 每行使用 `<Mahgen seq={mjaiToMahgen([wait.tile])} kind="rec" />`。
- 使用与 `RecommendationsTile` 相同的列表卡片样式。

价值：

- 这是当前 `Hand13Result` 已经计算但未直接展示的第一优先级数据。
- 对用户理解牌型速度最直接。

### 2. 切牌候选详情模块

目标：把前三推荐从“结论列表”扩展为“候选解释”。

数据来源：

- `result.hand14.maintain`
- 每个候选的 `discard`、`result.waits`、`waits_total`、`mixed_waits_score`、`avg_agari_rate`、`mixed_round_point`
- `result.best_attack_discard`
- `result.best_defence_discard`

展示方式：

- 表格或列表展示全部不退向候选。
- 列：切牌、速度评分、总进张、和牌率、EV、主要进张。
- 用标签标出“攻”和“守”：`best_attack_discard`、`best_defence_discard`。

实现方式：

- 可新建 `DiscardCandidatesTile.tsx`，也可扩展现有 `RecommendationsTile`。
- 仍沿用 `Mahgen + pct + toFixed` 的轻量实现。
- 为避免空间拥挤，默认只展示前 6 个候选，其余折叠或滚动。

价值：

- 能解释推荐来源，降低“AI 黑箱感”。
- 可以让用户看到攻守分歧，比如最快切牌和最安全切牌不一致。

### 3. 改良摸牌模块

目标：展示“摸到某些牌虽然不进向，但会让牌型变宽”的信息。

数据来源：

- `result.hand13.improves`
- `result.hand13.improve_way_count`
- `result.hand13.avg_improve_waits_count`

展示方式：

- 顶部显示改良方式数与平均改良后进张数。
- 每行：摸牌牌面、改良后总进张、改良后的等待列表。
- 可按 `widened_total` 降序排列。

实现方式：

- 新建 `ImprovesTile.tsx`。
- 每个 `ImproveEntry.draw` 用 `mjaiToMahgen([draw])` 渲染。
- `widened_waits` 可显示前若干张牌，鼠标悬停或展开时显示完整列表。

价值：

- 这部分是后端 13 张分析中相当有价值但完全未展示的数据。
- 对一向听、两向听的牌效理解尤其有帮助。

### 4. 下一向听速度热力模块

目标：把 `next_shanten_waits_count` 从数字映射变成 34 牌种热力图。

数据来源：

- `result.hand13.next_shanten_waits_count`
- `result.hand13.avg_next_shanten_waits`
- `TILE_LABELS_34`

展示方式：

- 34 格热力图，每格对应一种牌。
- 数值越高，背景越强；空值或 0 弱化。
- 顶部显示平均下一层进张数。

实现方式：

- 新建 `NextShantenHeatmapTile.tsx`。
- 可沿用 `RiskChartTile` 的 34 维数组遍历方式，但改成网格。
- 若要显示牌面，可先用文本标签；后续再改为小尺寸 `Mahgen`。

价值：

- 它能说明当前进张后，下一步牌型是否仍然宽。
- 与 `mixed_waits_score` 的解释关系最强。

### 5. 攻守对照模块

目标：将进攻推荐与防守推荐并排，辅助用户在高风险场景下选择。

数据来源：

- `result.best_attack_discard`
- `result.best_defence_discard`
- `result.mixed_risk`
- `result.hand14.maintain`
- `tileIdx()`

展示方式：

- 两个小面板：进攻切牌、防守切牌。
- 显示对应牌、风险值、切后进张/EV。
- 若两者相同，显示“攻守一致”；若不同，显示差异。

实现方式：

- 新建 `AttackDefenceTile.tsx`。
- 使用 `tileIdx(discard)` 从 `mixed_risk` 中取风险。
- 用 `hand14.maintain.find(c => c.discard === discard)` 补充进攻指标。

价值：

- 当前后端已经给出两个结论，但前端没有明确可视化。
- 实战中比单纯前三推荐更符合“局势判断”的需要。

### 6. 单对手风险矩阵模块

目标：把综合风险拆解到每个对手，帮助用户判断风险来源。

数据来源：

- `result.opponents[].risk`
- `result.opponents[].tenpai_rate`
- `result.opponents[].is_riichi`
- `TILE_LABELS_34`

展示方式：

- 行为牌种，列为对手，单元格显示风险或颜色。
- 也可只显示“当前手牌可切牌”的风险，降低密度。

实现方式：

- 新建 `OpponentRiskMatrixTile.tsx`。
- 若使用完整 34 x 3/4 矩阵，建议 CSS grid；若只展示候选切牌，可以复用 `Table`。

价值：

- 弥补 `RiskChartTile` 只看综合风险的不足。
- 对立直者、鸣牌者的风险差异更直观。

## 六、优先级建议

建议按以下顺序实现：

1. `WaitsTile`：数据稳定、实现最简单、价值最高。
2. `DiscardCandidatesTile`：直接增强现有推荐切牌解释力。
3. `AttackDefenceTile`：利用已有最佳攻守字段，实战决策价值高。
4. `ImprovesTile`：展示牌效深度，适合进阶用户。
5. `NextShantenHeatmapTile`：视觉效果强，但需要更仔细地设计密度。
6. `OpponentRiskMatrixTile`：信息量大，适合作为高级/可隐藏模块。

## 七、同实现方式的接入模板

新增模块推荐保持以下结构：

```tsx
import { useTranslation } from 'react-i18next'
import { TileFrame } from '@/components/TileFrame'
import { Mahgen } from '@/components/Mahgen'
import { useAnalysisStore } from '@/stores/analysisStore'
import { mjaiToMahgen } from '@/lib/tileIdx'
import type { Breakpoint } from '@/tiles/defaults'

export function ExampleAnalysisTile({ bp }: { bp: Breakpoint }) {
  const { t } = useTranslation()
  const result = useAnalysisStore((s) => s.result)

  return (
    <TileFrame id="example-analysis" title={t('tile.example_analysis')} bp={bp}>
      {!result ? (
        <span className="text-muted-foreground text-sm">{t('tile.analysis_empty')}</span>
      ) : (
        <Mahgen seq={mjaiToMahgen([result.best_attack_discard ?? ''])} kind="rec" />
      )}
    </TileFrame>
  )
}
```

接入清单：

- `frontend/src/tiles/defaults.ts`：扩展 `TileId`、`ALL_TILES`、默认布局、标题。
- `frontend/src/tiles/registry.tsx`：注册新组件。
- `frontend/src/components/AddTileMenu.tsx`：添加 i18n key 映射。
- `frontend/src/i18n/resources/en.json`、`zh-CN.json`、`zh-TW.json`、`ja.json`：添加标题与空态文案。
- 如需牌面：使用 `Mahgen` 与 `mjaiToMahgen`。
- 如需百分比：使用 `pct()`。
- 如需 34 维牌种：复用 `TILE_LABELS_34` 与 `tileIdx()`。

## 八、风险与注意事项

- Zustand selector 不应每次返回新数组；需要像 `OpponentsTile` 一样使用稳定空数组，避免 React 重渲染循环。
- 34 维数据在小尺寸磁贴内会很挤，建议默认只展示关键项，完整视图放在可滚动区域。
- `hand13` 与 `hand14` 互斥，组件必须按 `result.state` 或 null 值处理空态。
- `agari_rate` 在非听牌状态可能为 `null` 或无意义，显示时需用 `pct()` 兜底。
- `mixed_risk` 是按 34 维 Tile34 索引排列，必须通过 `tileIdx()` 或 `TILE_LABELS_34` 对齐，不能按字符串排序后直接索引。
- 若新增 Recharts 图表，应沿用历史页的 `ResponsiveContainer`、CSS 变量配色和 tooltip 样式，保证主题一致。

## 九、总体结论

当前 Akagi 的牌型分析后端已经具备较完整的数据面：进张、改良、切牌候选、速度评分、和牌率、点数期望、对手风险和综合风险都已通过 `AnalysisResult` 下发。前端实时可视化目前只覆盖了手牌、前三推荐、综合风险、对手摘要四类信息。

后续新增模块不需要重做分析管道，应沿用现有磁贴体系：`TileFrame` 负责统一容器和布局控制，`useAnalysisStore` 负责消费实时分析结果，`Mahgen` 负责牌面渲染，表格/CSS 条形条/Recharts 负责数值可视化。最值得优先补齐的是进张列表、切牌候选详情、攻守对照和改良摸牌四个模块。
