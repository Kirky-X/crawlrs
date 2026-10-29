#!/usr/bin/env python3
# Copyright (c) 2025-2026 Kirky.X🌠
# SPDX-License-Identifier: Apache-2.0

"""Criterion 基线回归门禁.

Criterion 本身只报告与基线的相对变化、不设进程退出码（criterion 0.8.2
``analysis`` 模块仅产出 change 报告），本脚本补齐确定性判定：
扫描 ``<criterion-dir>/**/new/estimates.json``，与同层级
``<baseline>/estimates.json`` 配对，任一基准的中位数点估计上涨超过
阈值即退出码 1（CI 超阈值失败）。

用法（与 .github/workflows/bench.yml 及 docs/PERFORMANCE.md 口径一致）::

    python3 scripts/bench_gate.py --criterion-dir target/criterion \
        --baseline ci-baseline --threshold-pct 20
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

EXIT_REGRESSION = 1
EXIT_NO_BASELINE = 2


def default_criterion_dir() -> Path:
    """与 cargo 同规则：`CARGO_TARGET_DIR` 优先，否则 `target/criterion`。"""
    target_dir = os.environ.get("CARGO_TARGET_DIR")
    base = Path(target_dir) if target_dir else Path("target")
    return base / "criterion"


def load_median(path: Path) -> float:
    with path.open(encoding="utf-8") as fh:
        estimates = json.load(fh)
    return float(estimates["median"]["point_estimate"])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--criterion-dir",
        type=Path,
        default=default_criterion_dir(),
        help="Criterion 输出目录（缺省按 CARGO_TARGET_DIR 解析，否则 target/criterion）",
    )
    parser.add_argument(
        "--baseline", required=True, help="对比的基线名（--save-baseline 存档名）"
    )
    parser.add_argument(
        "--threshold-pct",
        type=float,
        default=20.0,
        help="回归判定阈值：中位数上涨百分比（默认 20）",
    )
    args = parser.parse_args()

    criterion_dir: Path = args.criterion_dir
    if not criterion_dir.is_dir():
        print(f"::error::criterion 目录不存在: {criterion_dir}")
        return EXIT_NO_BASELINE

    new_runs = sorted(criterion_dir.glob("**/new/estimates.json"))
    if not new_runs:
        print(
            "::error::未找到本次运行的测量结果（**/new/estimates.json）"
            "——基准是否真的跑过？"
        )
        return EXIT_NO_BASELINE

    paired = 0
    regressions: list[tuple[str, float, float, float]] = []
    for new_est in new_runs:
        bench_dir = new_est.parent.parent
        base_est = bench_dir / args.baseline / "estimates.json"
        rel = bench_dir.relative_to(criterion_dir)
        if not base_est.is_file():
            print(
                f"::warning::无 {args.baseline} 基线可对照，跳过"
                f"（新增基准属正常，重建基线后纳入门禁）: {rel}"
            )
            continue
        paired += 1
        base_median = load_median(base_est)
        new_median = load_median(new_est)
        change_pct = (new_median - base_median) / base_median * 100.0
        verdict = "REGRESSION" if change_pct > args.threshold_pct else "ok"
        print(
            f"{verdict:>9}  {change_pct:+7.2f}%  base={base_median:>14.3f} ns  "
            f"new={new_median:>14.3f} ns  {rel}"
        )
        if change_pct > args.threshold_pct:
            regressions.append((str(rel), base_median, new_median, change_pct))

    if paired == 0:
        print(
            f"::error::没有任何基准能与基线 '{args.baseline}' 配对——"
            f"基线名不符或采样口径不一致；先以 mode=save 重建基线"
        )
        return EXIT_NO_BASELINE

    if regressions:
        print(f"\n{len(regressions)} 个基准超过阈值 {args.threshold_pct:.1f}%：")
        for rel, base_median, new_median, change_pct in regressions:
            print(
                f"  {rel}: {base_median:.3f} -> {new_median:.3f} ns ({change_pct:+.2f}%)"
            )
        return EXIT_REGRESSION

    print(f"\n全部 {paired} 个基准对比通过（阈值 {args.threshold_pct:.1f}%）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
