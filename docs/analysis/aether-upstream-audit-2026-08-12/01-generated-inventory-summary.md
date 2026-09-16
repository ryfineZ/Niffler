# 自动生成的全量差异清单摘要

> 本文件由 `generate_inventory.py` 从固定提交号生成，不包含人工处置结论。

## 固定基线

- 共同祖先：`ed75ae6d56ab03eb5e6e3cd87f2137880c99694d`
- Niffler 主线：`908443291a2826b57286f56f1555fd10e922c0b3`
- Aether 主线：`654c4f69789f02d08e926a77338f1b94f34f8658`
- Niffler 独有提交：357
- Aether 独有提交：776
- 两侧改动路径并集：3310

## 路径关系

| 类型 | 路径数 |
|---|---:|
| `niffler_only` | 634 |
| `aether_only` | 1977 |
| `both_changed_same` | 1 |
| `both_changed_diverged` | 698 |

## 初步冲突等级

| 等级 | 路径数 |
|---|---:|
| `critical` | 244 |
| `high` | 1411 |
| `medium` | 500 |
| `low` | 1154 |
| `aligned` | 1 |

## 子系统路径数量

| 子系统 | Niffler 独有 | Aether 独有 | 双方一致 | 双方分叉 |
|---|---:|---:|---:|---:|
| `data_storage` | 58 | 527 | 0 | 94 |
| `provider_protocol` | 67 | 452 | 0 | 118 |
| `usage_observability` | 61 | 172 | 1 | 54 |
| `billing_wallet` | 43 | 75 | 0 | 45 |
| `frontend_product` | 198 | 155 | 0 | 112 |
| `gateway_other` | 44 | 91 | 0 | 84 |
| `runtime_performance` | 3 | 48 | 0 | 11 |
| `backend_shared` | 2 | 167 | 0 | 11 |
| `routing_scheduler` | 5 | 104 | 0 | 50 |
| `auth_security` | 24 | 59 | 0 | 39 |
| `stream_execution` | 1 | 10 | 0 | 5 |
| `tests` | 10 | 32 | 0 | 36 |
| `documentation` | 58 | 8 | 0 | 5 |
| `delivery_operations` | 45 | 20 | 0 | 11 |
| `tunnel_frontdoor` | 5 | 36 | 0 | 14 |
| `repository_other` | 6 | 0 | 0 | 2 |
| `build_dependencies` | 4 | 21 | 0 | 7 |

## 生成附件

- `generated/path_inventory.tsv`：每个变更路径的双向状态、增删行、最终一致性、子系统和冲突等级。
- `generated/overlap_paths.tsv`：双方都修改过的路径。
- `generated/niffler_commits.tsv`：Niffler 全部独有提交。
- `generated/aether_commits.tsv`：Aether 全部独有提交。
- `generated/*_commit_impacts.tsv`：每个独有提交相对第一父提交的实际文件数、增删行和主要子系统。
- `generated/*_commit_catalog.tsv`、`generated/*_commit_decisions.tsv`：逐提交功能分类和处置建议。
- `generated/*_path_commit_map.tsv`、`generated/path_coverage_ledger.tsv`：逐路径历史来源和最终覆盖状态。
- `generated/*_decision_summary.tsv`：处置标签数量汇总。
- `generated/renames.tsv`：三组比较中 Git 可识别的重命名和复制。
- `generated/subsystem_summary.tsv`、`generated/component_summary.tsv`：聚合统计。
- `generated/metadata.json`：用于复核数量的一致性元数据。

> 大规模目录迁移时，Git 相似度算法可能把内容相近的旧文件和新文件配成重命名。完整性与数量复核一律以 `--no-renames` 的路径清单为准，`renames.tsv` 只用于人工辅助定位。
