#!/bin/sh
# Publish this repository's current commit to the public GitHub repo as release v<version>.
# The public repo has its own history: one commit per release, without docs/superpowers/ (the
# private planning notes). Pushing the tag starts the release build (.github/workflows/release.yml).
#   scripts/publish.sh 0.2.0
set -e
v="$1"
[ -n "$v" ] || { echo "usage: scripts/publish.sh <version, e.g. 0.2.0>"; exit 1; }
remote="${PUBLIC_REMOTE:-github}"
git diff --quiet && git diff --cached --quiet || { echo "commit your changes first"; exit 1; }
for f in Cargo.toml app/package.json app/src-tauri/tauri.conf.json; do
  grep -q "\"\?version\"\? *[:=] *\"$v\"" "$f" || { echo "$f doesn't say version $v; set the version there first"; exit 1; }
done
git rev-parse -q --verify "refs/tags/v$v" >/dev/null && { echo "v$v is released already"; exit 1; }
idx=$(mktemp)
GIT_INDEX_FILE="$idx" git read-tree HEAD
GIT_INDEX_FILE="$idx" git rm -r -q --cached --ignore-unmatch docs/superpowers .superpowers
tree=$(GIT_INDEX_FILE="$idx" git write-tree)
rm -f "$idx"
parent=$(git rev-parse -q --verify refs/heads/public || true)
msg=$(printf 'Index %s\n%s' "$v" "${PUBLISH_TRAILER:+
$PUBLISH_TRAILER}")
commit=$(git commit-tree "$tree" ${parent:+-p "$parent"} -m "$msg")
git update-ref refs/heads/public "$commit"
git tag -a "v$v" "$commit" -m "Index $v"
git push "$remote" public:main "v$v"
echo "Published v$v. The release build: https://github.com/YoruDev-Ryland/index-library/actions"
