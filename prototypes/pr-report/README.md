# PR report variants

Interactive comparison of candidate `code-graph` PR-comment formats, built from real
snapshots of GoatXQ/application backend PRs.

## Data

`/tmp/cgpr/<pr>/` holds, per PR:

- `base.json` — snapshot of `git merge-base <PR head> origin/main`, analyzed at `backend/src`
- `head.json` — snapshot of the PR head
- `diff.json` — `code-graph audit diff --format json --changed-files changed.txt`
- `changed.txt` — changed paths relative to `backend/src`

Both sides run with `--max-constants 5000`: the default cap of 500 makes the set of
crawled constants unstable between runs, which shows up as fake diffs.

## Scripts

```bash
python3 analyze.py            # noise table across all built PRs
python3 verify.py 269 6       # print the source behind each claim, for eyeball checks
python3 generate.py           # rebuild index.html for every built PR
python3 generate.py 255 273   # rebuild a subset
```

`index.html` embeds everything (no network): pick a PR, pick a variant, optionally
compare against another variant. Arrow keys move between variants.
