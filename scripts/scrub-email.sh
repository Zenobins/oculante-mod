#!/usr/bin/env bash
#
# 把历史提交里的个人邮箱替换成 GitHub noreply 地址。
#
#   ./scripts/scrub-email.sh            # 只统计，不改动（默认）
#   ./scripts/scrub-email.sh --apply    # 真正重写历史（不可逆！先读下面的说明）
#
# 说明：
#  * 只改 author/committer 的 name+email，不动文件内容与提交信息。
#  * 重写会改变所有提交的 SHA：分支需要 force-push，tag 需要重建。
#  * 远端旧对象不会立刻消失（GitHub 有缓存，其他人/上游 PR 里的副本仍在），
#    要彻底清除还需要联系 GitHub Support 清理缓存，并清理/通知 fork。
#  * 本脚本不自动 push，重写后请自己核对再推。
#
set -euo pipefail

OLD_MAIL="little-ake@outlook.com"
NEW_NAME="Zenobins"
NEW_MAIL="19223568+Zenobins@users.noreply.github.com"
MODE="${1:-}"

git rev-parse --is-inside-work-tree >/dev/null

COUNT=$(git log --all --format='%ae%n%ce' | grep -c -F "$OLD_MAIL" || true)
echo "包含 $OLD_MAIL 的作者/提交者条目：$COUNT"

if [ "$COUNT" -eq 0 ]; then
    echo "没有需要处理的提交。"
    exit 0
fi

if [ "$MODE" != "--apply" ]; then
    echo
    echo "预览（不改动）："
    git log --all --format='%h %ae %s' | grep -F "$OLD_MAIL" | head -10
    echo
    echo "确认无误后执行：./scripts/scrub-email.sh --apply"
    exit 0
fi

if [ -n "$(git status --porcelain)" ]; then
    echo "工作区不干净，先提交或 stash。" >&2
    exit 1
fi

# 重写前的本地备份分支（注意：filter-repo 会重写所有 ref，所以它只是心理安慰，
# 真正的"原件"在远端 —— 只要还没 force-push，远端仍是旧历史）。
git branch -f "backup/pre-scrub-$(date +%Y%m%d-%H%M)" HEAD >/dev/null 2>&1 || true

MAP="$(git rev-parse --git-dir)/scrub-mailmap"
printf '%s <%s> <%s>\n' "$NEW_NAME" "$NEW_MAIL" "$OLD_MAIL" > "$MAP"
echo "mailmap: $MAP"
cat "$MAP"

FILTER_REPO="${FILTER_REPO:-$HOME/tools/git-filter-repo}"
if [ ! -x "$FILTER_REPO" ]; then
    echo "找不到 git-filter-repo（$FILTER_REPO）。" >&2
    echo "下载：curl -fsSL -o $FILTER_REPO https://raw.githubusercontent.com/newren/git-filter-repo/main/git-filter-repo && chmod +x $FILTER_REPO" >&2
    exit 1
fi

"$FILTER_REPO" --mailmap "$MAP" --force

# filter-repo 出于安全会移除所有 remote，按需加回来
git remote add origin https://github.com/woelper/oculante.git 2>/dev/null || true
git remote add fork https://github.com/Zenobins/oculante-mod.git 2>/dev/null || true
git fetch origin master --quiet || true

echo
echo "重写完成。现在的作者邮箱："
git log --all --format='%ae' | sort -u | sed 's/^/  /'

cat <<'EOF'

下一步（手动，确认后再执行）：

  git push --force-with-lease fork feat/input-remap
  # tag 也要重建：
  git tag -f -a vX.Y.Z-mod.<stamp> -m "..." && git push -f fork vX.Y.Z-mod.<stamp>

别忘了：
  1) GitHub → Settings → Emails：勾选 "Keep my email addresses private"
     和 "Block command line pushes that expose my email"；
  2) 已经存在的旧对象/缓存，需要 GitHub Support 协助清理；
  3) 上游 PR、其他人的 fork 里的副本无法由我们改写。
EOF
