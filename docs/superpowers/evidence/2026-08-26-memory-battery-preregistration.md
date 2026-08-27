# `memory-battery-v1` — pre-registration (committed BEFORE any GPU run)

**Date:** 2026-08-26/27 (past-midnight continuation of the same working
session that shipped the spec and Tasks 1–4; corpus generation and this
lock both happen under the 2026-08-26 date prefix used throughout this
project's files). **Branch:** `memory-battery` @ `99bc54b` (worktree
`.worktrees/memory-battery`), the tip after Task 4's review-clean commit —
**no code changes ride with this commit**; `git diff --stat` against
`99bc54b` for this lock touches only `tools/memory_battery/corpus-v1/`
(new, committed) and this document. **Spec:**
`docs/superpowers/specs/2026-08-26-memory-battery-design.md` — binding;
§4's formulas are cited below, never restated with different words (the
plan's own rule, `docs/superpowers/plans/2026-08-26-memory-battery.md`).
**Amendment protocol:** identical to `docs/gates.md`'s house rule (all
values below are frozen; changes require a recorded protocol amendment
executed before re-running, never tune-and-rerun) and spec §6's own
non-silent amendment rule (dated footnote, never an in-place edit). Any
post-commit amendment to this document is a **separate dated file**, never
an edit of this one.

## 1. Claim discipline (spec §1, quoted verbatim)

> The gate's verdict licenses exactly one sentence per outcome: PASS — "on
> `memory-battery-v1`, exact-repeat injection reduced median second-exposure
> completion-token cost by the measured amount"; FAIL — the same sentence
> with "did not reduce ... beyond the derived bar"; UNMEASURABLE / INVALID
> — the named reason and no cost claim at all. Nothing about novel tasks,
> other models, other task shapes, or accuracy. Per the house rule, the
> point estimate decides: no extension, no re-run, no corpus change after
> any gate number is read. A run killed by infrastructure with **no gate
> number read** may be rerun in full from zero; partial data is never
> spliced.

This instrument (`memory-battery-v1`) is its own frozen instrument — the
G4/G5 sets are untouched and stay memory-off (spec §2). Out-of-scope items
(spec §7 — accuracy on hardened families, any second model, any organ code
change, non-exact retrieval, per-task paired toggling, wall-clock as a
gate, success-rate floors) are named so their absence from this document is
a decision, not an oversight.

## 2. Lens (spec §2 pins)

| pin | value | source |
|---|---|---|
| model | `qwen36-reap48-flywheel5-Q4_K_M.gguf` | `driver.py` `MODEL` constant |
| model digest (`expected_digest`) | `7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd` | pinned here; matches the GGUF sha256 measured at training (`2026-08-23-flywheel5-training.md`) and the served digest measured at the memory-organ slice-1 acceptance (`2026-08-26-memory-organ-acceptance.md`) |
| envelope | `v4` | `recompute.py` `ENVELOPE` constant |
| `window_cap` | `16384` (every battery agent) | `driver.py` `WINDOW_CAP` constant |
| poll cadence | `5.0` s | `driver.py` `DEFAULT_POLL_INTERVAL_S` |
| per-task poll deadline | `600.0` s | `driver.py` `DEFAULT_TASK_DEADLINE_S` |
| arm order | **C then M** (fixed, kills the ordering degree of freedom — spec status line) | spec header |
| corpus seed | `20260826` | manifest `corpus_seed` |
| bootstrap seed | `20260826` | `recompute_bootstrap.py` `BOOTSTRAP_SEED` |
| bootstrap B | `10,000` | `recompute_bootstrap.py` `BOOTSTRAP_B` |
| hygiene/E1 SE multiplier | `2` (`2 × SE_boot`) | `recompute_bootstrap.py` `HYGIENE_SE_MULTIPLIER` |
| H3 infra-rate ceiling | `0.05` (5%) | `recompute_bootstrap.py` `INFRA_RATE_CEILING` |
| daemon commit | the **merged master tip's featured build** at run time — no sha exists yet; recorded in the findings doc at boot (spec §2: "daemon: the merged master tip's featured build (commit recorded)"). This branch may merge into master before Tasks 6/7 run, so the commit is necessarily read at boot time, not pinned here. |

### Boot configs, VERBATIM (adapted from the memory-organ slice-1 acceptance pattern)

Both configs below are the slice-1 live-acceptance boot config
(`.superpowers/sdd/2026-08-26-memory-organ/acceptance/bloomery.toml`, read
in full for this lock) with exactly three fields changed per arm — `port`,
`data_dir`, and `[memory].enabled` — everything else, including the model
stanza, carried verbatim. Each arm's `data_dir` is a fresh scratch
directory under this plan's own SDD tree; the production drift baseline at
`~/.local/share/bloomery` is never touched by either boot.

**Arm C** (`[memory] enabled = false`, port `8396`):

```toml
port = 8396
data_dir = "/home/brice/workspace/bloomery/.superpowers/sdd/2026-08-26-memory-battery/runs/arm-c/data"
tasks_enabled = true
ctx_overhead_mib = 512

[memory]
enabled = false

[models."qwen36-reap48-flywheel5"]
path = "/home/brice/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf"
envelope = "v4"
g5_probe = true

[tier]
name = "enthusiast-16gb"
emulated = false

[assay]
enabled = true
python = "python3"
probe_timeout_secs = 1800
```

**Arm M** (`[memory] enabled = true`, store starts EMPTY in this fresh
`data_dir`, port `8395`):

```toml
port = 8395
data_dir = "/home/brice/workspace/bloomery/.superpowers/sdd/2026-08-26-memory-battery/runs/arm-m/data"
tasks_enabled = true
ctx_overhead_mib = 512

[memory]
enabled = true

[models."qwen36-reap48-flywheel5"]
path = "/home/brice/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf"
envelope = "v4"
g5_probe = true

[tier]
name = "enthusiast-16gb"
emulated = false

[assay]
enabled = true
python = "python3"
probe_timeout_secs = 1800
```

Neither `runs/arm-c/data` nor `runs/arm-m/data` exists yet — both are
created fresh at Task 6/7 boot time, per arm, per spec §4 ("Arm M: ...
store starts EMPTY in a fresh scratch `data_dir`").

## 3. Corpus (spec §3)

### 3.1 Generation and structural check

Generated via the pinned entry point, run once from the worktree root:

```
python3 -c "
from pathlib import Path
from tools.memory_battery.corpus import generate_corpus
generate_corpus(seed=20260826, n=50, out_dir=Path('tools/memory_battery/corpus-v1'))
"
```

50 tasks drawn, 8 run-verified python families (`families` from the
manifest): `py_inverted_boolean_run_verified` ×7,
`py_off_by_one_index_run_verified` ×7,
`py_off_by_one_range_bound_run_verified` ×6,
`py_wrong_comparison_operator_run_verified` ×6,
`py_wrong_constant_multiplier_run_verified` ×6,
`py_wrong_dict_key_run_verified` ×6, `py_wrong_fstring_field_run_verified`
×6, `py_wrong_variable_reference_run_verified` ×6.

`corpus_check` (`python3 -m tools.memory_battery.corpus_check
tools/memory_battery/corpus-v1`), executed, not asserted (spec §3's
black-oxide rule) — full output, verbatim, exit code `0`:

```
task                                            fails_before  passes_after  sha256
----------------------------------------------------------------------------------
py_inverted_boolean_run_verified-0000           PASS          PASS          PASS
py_off_by_one_index_run_verified-0001           PASS          PASS          PASS
py_off_by_one_range_bound_run_verified-0002     PASS          PASS          PASS
py_wrong_comparison_operator_run_verified-0003  PASS          PASS          PASS
py_wrong_constant_multiplier_run_verified-0004  PASS          PASS          PASS
py_wrong_dict_key_run_verified-0005             PASS          PASS          PASS
py_wrong_fstring_field_run_verified-0006        PASS          PASS          PASS
py_wrong_variable_reference_run_verified-0007   PASS          PASS          PASS
py_inverted_boolean_run_verified-0008           PASS          PASS          PASS
py_off_by_one_index_run_verified-0009           PASS          PASS          PASS
py_off_by_one_range_bound_run_verified-0010     PASS          PASS          PASS
py_wrong_comparison_operator_run_verified-0011  PASS          PASS          PASS
py_wrong_constant_multiplier_run_verified-0012  PASS          PASS          PASS
py_wrong_dict_key_run_verified-0013             PASS          PASS          PASS
py_wrong_fstring_field_run_verified-0014        PASS          PASS          PASS
py_wrong_variable_reference_run_verified-0015   PASS          PASS          PASS
py_inverted_boolean_run_verified-0016           PASS          PASS          PASS
py_off_by_one_index_run_verified-0017           PASS          PASS          PASS
py_off_by_one_range_bound_run_verified-0018     PASS          PASS          PASS
py_wrong_comparison_operator_run_verified-0019  PASS          PASS          PASS
py_wrong_constant_multiplier_run_verified-0020  PASS          PASS          PASS
py_wrong_dict_key_run_verified-0021             PASS          PASS          PASS
py_wrong_fstring_field_run_verified-0022        PASS          PASS          PASS
py_wrong_variable_reference_run_verified-0023   PASS          PASS          PASS
py_inverted_boolean_run_verified-0024           PASS          PASS          PASS
py_off_by_one_index_run_verified-0025           PASS          PASS          PASS
py_off_by_one_range_bound_run_verified-0026     PASS          PASS          PASS
py_wrong_comparison_operator_run_verified-0027  PASS          PASS          PASS
py_wrong_constant_multiplier_run_verified-0028  PASS          PASS          PASS
py_wrong_dict_key_run_verified-0029             PASS          PASS          PASS
py_wrong_fstring_field_run_verified-0030        PASS          PASS          PASS
py_wrong_variable_reference_run_verified-0031   PASS          PASS          PASS
py_inverted_boolean_run_verified-0032           PASS          PASS          PASS
py_off_by_one_index_run_verified-0033           PASS          PASS          PASS
py_off_by_one_range_bound_run_verified-0034     PASS          PASS          PASS
py_wrong_comparison_operator_run_verified-0035  PASS          PASS          PASS
py_wrong_constant_multiplier_run_verified-0036  PASS          PASS          PASS
py_wrong_dict_key_run_verified-0037             PASS          PASS          PASS
py_wrong_fstring_field_run_verified-0038        PASS          PASS          PASS
py_wrong_variable_reference_run_verified-0039   PASS          PASS          PASS
py_inverted_boolean_run_verified-0040           PASS          PASS          PASS
py_off_by_one_index_run_verified-0041           PASS          PASS          PASS
py_off_by_one_range_bound_run_verified-0042     PASS          PASS          PASS
py_wrong_comparison_operator_run_verified-0043  PASS          PASS          PASS
py_wrong_constant_multiplier_run_verified-0044  PASS          PASS          PASS
py_wrong_dict_key_run_verified-0045             PASS          PASS          PASS
py_wrong_fstring_field_run_verified-0046        PASS          PASS          PASS
py_wrong_variable_reference_run_verified-0047   PASS          PASS          PASS
py_inverted_boolean_run_verified-0048           PASS          PASS          PASS
py_off_by_one_index_run_verified-0049           PASS          PASS          PASS
----------------------------------------------------------------------------------
families                                        PASS
----------------------------------------------------------------------------------
OVERALL: PASS
```

**4/4 checks (fails-before, passes-after, `workspace_sha256`, families) on
all 50 tasks — every check EXECUTED, per spec §3.** Since `workspace_sha256`
(check 3) requires `workspace/`'s bytes and `pristine/`'s bytes to hash to
the *same* manifest value, this table also stands as the executed proof
that `workspace/ == pristine/` byte-for-byte for all 50 tasks — carried
into the freeze sha derivation below, which therefore does not separately
walk `pristine/`.

### 3.2 Freeze sha

Per spec §6 step 2 ("sha256 over the sorted manifest + workspace bytes").
sha256 over `manifest.json`'s bytes, then every file under
`tasks/<name>/workspace/`, in sorted relative-path order, each path and its
bytes NUL-separated on both sides (the same pairing convention `corpus.py`'s
`_workspace_sha256` and `corpus_check.py`'s independent reimplementation
both use). `pristine/` is not separately walked — §3.1 above is the
executed proof it is byte-identical to `workspace/` for every task, so
hashing it too would only restate the same bytes under a different
directory name. Runnable from the worktree root:

```python
import hashlib
from pathlib import Path

def freeze_sha256(corpus_dir: Path) -> str:
    hasher = hashlib.sha256()
    hasher.update((corpus_dir / "manifest.json").read_bytes())
    for path in sorted(corpus_dir.glob("tasks/*/workspace/*")):
        rel = path.relative_to(corpus_dir).as_posix()
        hasher.update(rel.encode("utf-8") + b"\0")
        hasher.update(path.read_bytes() + b"\0")
    return hasher.hexdigest()

print(freeze_sha256(Path("tools/memory_battery/corpus-v1")))
```

**`freeze_sha256 = d9df82e2f7ae95130fc8fa765b5b1faff7b15e93832f8adfd1980b07d797c9d5`**
(100 workspace files hashed = 50 tasks × 2 files each — the planted-defect
module and its `unittest`; run twice, byte-identical both times). **After
this commit the corpus is bytes, not code** (spec §3): any post-lock
change to `tools/memory_battery/corpus-v1/` is a dated footnote amendment
in a separate file, never an edit to the committed tree or to this
document.

For cross-check at gate time only (**not** a substitute for the freeze sha
above — `recompute.py`'s own docstring on `_corpus_sha` names this
distinction explicitly: it derives purely from the frozen manifest's own
per-task `workspace_sha256` values, with no live filesystem re-read, and is
recomputed fresh by every `recompute()` call): `recompute`'s emitted
`lens.corpus_sha` is expected to read
`778b1491aac67f9235ff2ae6ce74c0c767465fb30b2ab5053e17ce99ccc9a5ff` for this
corpus, unchanged for as long as the corpus is unchanged.

### 3.3 Per-task `workspace_sha256` (50 rows, from the manifest)

| task | family | `workspace_sha256` |
|---|---|---|
| py_inverted_boolean_run_verified-0000 | py_inverted_boolean_run_verified | `c1cbe528f17a359f32302e42688633c1f47678aad063b9ea797a845399a8f186` |
| py_off_by_one_index_run_verified-0001 | py_off_by_one_index_run_verified | `6f37a9d46e3d831d9075f04bbdce5e13afb2b5cf2d300661f895efe7f957d1be` |
| py_off_by_one_range_bound_run_verified-0002 | py_off_by_one_range_bound_run_verified | `fc35cf352d6c2b59f6baec4890245d0c133e460c8081b7880021e59cff27dd2e` |
| py_wrong_comparison_operator_run_verified-0003 | py_wrong_comparison_operator_run_verified | `6a95b08150a8d24cd89797106607044e053b0ec045da167fbfe5d333b2040dc0` |
| py_wrong_constant_multiplier_run_verified-0004 | py_wrong_constant_multiplier_run_verified | `8ec4717c22cb78b81daabbf1e35ae4de35c5e4c3785b75d9143b27314be805f3` |
| py_wrong_dict_key_run_verified-0005 | py_wrong_dict_key_run_verified | `46a3c48b8460b4db417fd926b593d3fc4c769b4f5203dfcfe958c8bdef357867` |
| py_wrong_fstring_field_run_verified-0006 | py_wrong_fstring_field_run_verified | `b65a7e25b5e6b89d44f98d54a01b5e3a32e06f2237a60414b980f8ee7ff99e89` |
| py_wrong_variable_reference_run_verified-0007 | py_wrong_variable_reference_run_verified | `95ad1faaaade62a5501bac26d9a0c5b00c930888aafeb679ecd3d522ccf86535` |
| py_inverted_boolean_run_verified-0008 | py_inverted_boolean_run_verified | `e513f59d4b9b5f17bbc67fa47dc2bd985f4558a0d6e22f2f5cd77135678f518d` |
| py_off_by_one_index_run_verified-0009 | py_off_by_one_index_run_verified | `67601087c18768b0e1b58f879404f73ca3baef5aa666b99bfef0f9934d54fec5` |
| py_off_by_one_range_bound_run_verified-0010 | py_off_by_one_range_bound_run_verified | `72396f0b677657f9d56eb4c04af19f5d12b596b1fffb19557186c00181d28570` |
| py_wrong_comparison_operator_run_verified-0011 | py_wrong_comparison_operator_run_verified | `54749ad555484eadd553d44faab3d018c333104dc097aef76d40d735dd5ffc97` |
| py_wrong_constant_multiplier_run_verified-0012 | py_wrong_constant_multiplier_run_verified | `b3ae6300e7450df4fe2156e46ee29e56709c2bf0b2e59feb0f7ec1f1275d93d7` |
| py_wrong_dict_key_run_verified-0013 | py_wrong_dict_key_run_verified | `88beafc1f93779d0fe5b0dbe13ba63db3e5e598f99d6f8b4160bba3221d0d7a4` |
| py_wrong_fstring_field_run_verified-0014 | py_wrong_fstring_field_run_verified | `3fc27b753c323464b58a509f382853712f69e267f46752434480974891153343` |
| py_wrong_variable_reference_run_verified-0015 | py_wrong_variable_reference_run_verified | `596b1861596db4071855c2328aebcfdb326d31ba91d038a1c7a5f2549042bc68` |
| py_inverted_boolean_run_verified-0016 | py_inverted_boolean_run_verified | `cdd72cf627dc11108c0a0bdf50d7aab91a5bb7894423e3d6700be1750e0910a4` |
| py_off_by_one_index_run_verified-0017 | py_off_by_one_index_run_verified | `2fefdf374c1745a94d5562d6253540d03e583d8a1e34cf29c7f878f9e0131ef2` |
| py_off_by_one_range_bound_run_verified-0018 | py_off_by_one_range_bound_run_verified | `67dc5dd1b763c4e6cd2940656dd35a451f2b4f805b03c97a447bc5faae6f8231` |
| py_wrong_comparison_operator_run_verified-0019 | py_wrong_comparison_operator_run_verified | `d6f8c17fdfc31a34a412aeb9a67453e0f5f9e15a8b7a678f1dfc636596083f01` |
| py_wrong_constant_multiplier_run_verified-0020 | py_wrong_constant_multiplier_run_verified | `e28b829ca126d9e4522104f6e724aa177b4bb2126be745f9c111dce6f1379b6b` |
| py_wrong_dict_key_run_verified-0021 | py_wrong_dict_key_run_verified | `daa93362fba1bd812aa95399552ebb9ab46c372acbf0ee16fe30270823427de2` |
| py_wrong_fstring_field_run_verified-0022 | py_wrong_fstring_field_run_verified | `31711c0b227c26a8db3a2062b6c729c6e59f3741052426a5fe7f8f50573d9df8` |
| py_wrong_variable_reference_run_verified-0023 | py_wrong_variable_reference_run_verified | `0aef2bebfb2bafa3b5bcc4645207d39c8dead2e901dbeabe46801b671822d646` |
| py_inverted_boolean_run_verified-0024 | py_inverted_boolean_run_verified | `89e0ab8306e587105db7f0d0d70c2c24d081648d6b81af65f0740b753bfcb3dc` |
| py_off_by_one_index_run_verified-0025 | py_off_by_one_index_run_verified | `6a6ef7969a283e8753d69f5931aa917237d0520f5bfcc4cf4a9c13708dca8d17` |
| py_off_by_one_range_bound_run_verified-0026 | py_off_by_one_range_bound_run_verified | `2be83b8a4b9fe10b48be3d6a6a4b27a3e648c3a0c3e85bc4a1e2a8e6b42c7a18` |
| py_wrong_comparison_operator_run_verified-0027 | py_wrong_comparison_operator_run_verified | `ef12ff1218c2cf14b22e8d2f11ac35551af29046bc74aadabfa49ce55300ec50` |
| py_wrong_constant_multiplier_run_verified-0028 | py_wrong_constant_multiplier_run_verified | `778a7b28897408eee3f3ba81d232aa3381c895e47c4baecabe01ecbeaf3c81d1` |
| py_wrong_dict_key_run_verified-0029 | py_wrong_dict_key_run_verified | `ffb6938e61b94491bfe0499f482537845655509d4e1870e830a7745ab9d1f4c9` |
| py_wrong_fstring_field_run_verified-0030 | py_wrong_fstring_field_run_verified | `1d30063a84688e7112e2c81cf465f9d3f86e01d272107421c5242a930a873652` |
| py_wrong_variable_reference_run_verified-0031 | py_wrong_variable_reference_run_verified | `6985a9bc09849f53255e44f102c6460d430824d531510a2a74750ca0917baaf6` |
| py_inverted_boolean_run_verified-0032 | py_inverted_boolean_run_verified | `4e5d0167ec2029eace1beb15d9025901b69632543388e18232d0d2b8b75ec042` |
| py_off_by_one_index_run_verified-0033 | py_off_by_one_index_run_verified | `dda29ba12e44f70f4ac06c11fd8dc83fda7257214a42cb7d7bd6b2caaae2b4e5` |
| py_off_by_one_range_bound_run_verified-0034 | py_off_by_one_range_bound_run_verified | `7fa74882b6ffa2d9d38d3d9891710f5d6e5760ee5181954f6681e99b888d5eff` |
| py_wrong_comparison_operator_run_verified-0035 | py_wrong_comparison_operator_run_verified | `d6807dc5e5cdc8109be09728fcccb65ca1f0ce63d6f7b2cb8d6727445ac4345d` |
| py_wrong_constant_multiplier_run_verified-0036 | py_wrong_constant_multiplier_run_verified | `6d7f5863633a3321136816751ee15d5f46e17ab52d44af26191436bb9528e407` |
| py_wrong_dict_key_run_verified-0037 | py_wrong_dict_key_run_verified | `7a83a74f466ea50931bc1eb7d0b5726126763cb5f82d47cd5e1a810aa3d7c65d` |
| py_wrong_fstring_field_run_verified-0038 | py_wrong_fstring_field_run_verified | `c5fe2be53623bb21572c731f82d49715e86a5923a83b53388a7a6c16ccad9401` |
| py_wrong_variable_reference_run_verified-0039 | py_wrong_variable_reference_run_verified | `7c8877a1b94508b4abbbd8690959477c12757f4c35c0810211b15b4a7b8f4caa` |
| py_inverted_boolean_run_verified-0040 | py_inverted_boolean_run_verified | `feb1a82975a773e542259ac7b2c44b5cb9dc080f6d6c02a67ec066ba19edb3ef` |
| py_off_by_one_index_run_verified-0041 | py_off_by_one_index_run_verified | `735801a80e5f41c924919e91d2534e65c91e627c0f93c16d52fb133ce0a5bc11` |
| py_off_by_one_range_bound_run_verified-0042 | py_off_by_one_range_bound_run_verified | `783b4cc1e7caf8b299adadbcbb012d6b11c473a039459fa47d35a230458cf375` |
| py_wrong_comparison_operator_run_verified-0043 | py_wrong_comparison_operator_run_verified | `10675523dc4a1dcfc210319a18c55c257ff29bc5af97686dbee0acdbe2021f82` |
| py_wrong_constant_multiplier_run_verified-0044 | py_wrong_constant_multiplier_run_verified | `fa1013bc1bfc05e3693b4517906d53c3a8f2aecefdf8dcf1c880d85f861545f4` |
| py_wrong_dict_key_run_verified-0045 | py_wrong_dict_key_run_verified | `98988c0f59cf5c5164ded216517adf26ec448962e87636ccf399e0ffa795e823` |
| py_wrong_fstring_field_run_verified-0046 | py_wrong_fstring_field_run_verified | `a04a79616f351ea27fee4148ac4ab135fc5743a9e05e5dc979f5a9ff475b2d9a` |
| py_wrong_variable_reference_run_verified-0047 | py_wrong_variable_reference_run_verified | `282f65d60f24a8724f7cfa6baa1fa314d0b8358a931fba96d378a584fbe7e679` |
| py_inverted_boolean_run_verified-0048 | py_inverted_boolean_run_verified | `de32d11920f26197617bee4d0ff289bad04b7dc477eddf6c68b220bc60366e2b` |
| py_off_by_one_index_run_verified-0049 | py_off_by_one_index_run_verified | `b2ce87b1a0bc50686b114decf7872bb8fa21243739b7a752ffefae6bcfa8262c` |

## 4. Protocol, endpoints, bars — by reference (spec §4)

Every formula (E1's gate condition and `Δ_min`, the headroom clause, H1–H4,
the advisory endpoints, `cost(task)`, the ITT/none-vs-zero rule, the
resampling-unit rules) is spec §4's own text, cited here, never restated
with different words. Two binding footnotes on how recompute (as built,
review-clean at `99bc54b`) resolves ordering questions spec §4's prose
leaves implicit:

**Footnote 1 — the headroom clause is evaluated BEFORE PASS/FAIL, not
after** (dated 2026-08-26, controller carry-note from Task 4's review).
`recompute_bootstrap.py`'s `_check_e1` computes `headroom = median_C,p2 -
min_C,p2` and `delta_min`, and returns verdict `UNMEASURABLE` the instant
`headroom < delta_min` — **before** the `median_M,p2 <= median_C,p2 -
delta_min` comparison that would otherwise decide PASS/FAIL ever runs. The
practical consequence: a very large true effect (`median_M,p2` sitting
below `min_C,p2`, i.e. beating every phase-2 control task's cost outright)
still reports **UNMEASURABLE**, never PASS, whenever the control
distribution is floor-saturated. This is the conservative, declared-in-
advance reading of spec §4's headroom clause (crucession E3b's own lesson,
named in the spec's lineage) — the gate never reports a PASS it cannot
distinguish from floor-saturation, no matter how large the raw gap looks.

**Footnote 2 — H3's denominator is `n × 2` task-halves per arm, the more
permissive of two readings** (dated 2026-08-26, controller carry-note from
Task 4's review). Spec §4 states H3 as "infra rate ≤ 5% per arm
(task-level: ...)" without pinning whether "task" means one task (counted
once) or one task-half (counted once per phase). `recompute_bootstrap.py`'s
`_check_h3` fixes the denominator at `n * 2` — 100 task-halves per arm at
`n=50` — the reading that gives infra the most room before a 5% kill
fires: at `n=50`, `n`-only would allow 2 infra tasks before a kill (5% of
50 = 2.5); `n×2` allows 5 (5% of 100 = 5) — **≈5 halves of slack at
N=50**, all of it in the direction of NOT killing a run over infra noise
that the ITT/none-vs-zero rule already prices out of the cost endpoint.

## 5. Machinery — file shas at lock (`git hash-object`, `tools/memory_battery/`)

```
$ for f in tools/memory_battery/*.py; do echo "$(git hash-object "$f")  $f"; done
```

| file | `git hash-object` sha |
|---|---|
| `__init__.py` | `e69de29bb2d1d6434b8b29ae775ad8c2e48c5391` |
| `corpus.py` | `070b070cb93f1b2af6de70a5441c1a5133507f02` |
| `corpus_check.py` | `59da8583af39f8a61fd4a524548f5b3ca1bbd9d2` |
| `driver.py` | `3ba696eb38c1a7d4903a59f9530115f207df8d2e` |
| `recompute.py` | `e5c9b7e64e590138368728444d878ad068246246` |
| `recompute_bootstrap.py` | `ad848336fcf4ebd7f27ac655753348885a8d81ab` |
| `recompute_join.py` | `781f77adfa7508a9e092c83fb3dee50867a953e3` |
| `recompute_journal.py` | `9c10a9f67fca4ae2bbe68b3d84ecc22e240fd10e` |

All eight files are unmodified at branch tip `99bc54b` (`git status
--short` clean against them at lock time); 64/64 package tests green
(`python3 -m unittest discover -s tools/memory_battery/tests`, run
immediately before this lock, `PYTHONDONTWRITEBYTECODE=1`).

## 6. The recompute CLI's `--expected-digest` requirement (carry-note, dated 2026-08-26)

`recompute.py`'s `main()` (the CLI Task 8 will invoke) declares
`--expected-digest` `required=True` — a real gate run cannot omit the
identity check (review finding I3: "the CLI enforces this because a real
gate run must never silently skip it; the library `recompute()` kwarg
stays optional-None so fixtures/tests that don't care about identity can
omit it"). Task 8's invocation **must** pass this document's digest pin:

```
--expected-digest 7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd
```

**Identity rows: 2 per arm run, one per phase** (`driver.py`'s
`_assert_identity`, called once before each phase's first task — plan
progress.md's own correction of an earlier arithmetic slip: "the correct
reading ... is one identity row per phase assert = 2 per `run_arm` call, 4
across the two-arm battery"). `recompute`'s `_check_identity` requires both
of an arm's identity rows to agree with each other AND (when
`--expected-digest` is supplied, as Task 8's CLI invocation always will)
with this pin; either arm's mismatch makes the whole run's verdict
**INVALID** — R-PF-B1 (amended), the plan's own ruling.

## 7. Operational preconditions (named so the human-gated runs do not discover them live)

- **Boot exactly ONE model.** The driver's `POST /agents` body names
  `MODEL` (`driver.py`'s module constant, §2 above) and the daemon's own
  `/status` identity assert reads `models[0]["digest"]` — a boot config
  listing more than one `[models."..."]` stanza is untested territory for
  this driver and is a preflight precondition for Tasks 6/7, not something
  the driver itself checks or enforces (plan progress.md Task 3 carry-note:
  "`models[0]` positional (boot exactly one model = operational
  precondition, note for T6/T7 preflight)"). Both boot configs above
  declare exactly one `[models."qwen36-reap48-flywheel5"]` stanza.
- **On wrapper-SIGKILL, the ledger is the authority — never eyeball it.**
  `run_battery.sh`'s trap can record a normal exit code or the
  `killed-by-signal` sentinel for every death mode except SIGKILL, which no
  trap can observe at all (`run_battery.sh`'s own docstring). A run that
  dies without ever writing `driver.DONE` is not automatically a failed
  arm: the driver's ledger is append-only and flushed after every row
  (`driver.py`'s `Ledger`), so a DIED-WITHOUT-MARKER run whose ledger shows
  both phases' full `2n` task-half rows is still a **completed** arm in
  every way `recompute` can measure. Per spec §6, the classification is
  never made by eye — it is exactly `recompute`'s own arm-completeness
  check (`recompute_bootstrap.py`'s `_check_arm_completeness`, evaluated
  first, ahead of identity/H1/H2/H3: `actual_task_halves != 2 * n` marks
  the arm hygiene-INVALID regardless of what killed the wrapper). Tasks 6/7
  archive the ledger and journal regardless of how the process ended, and
  Task 8's `recompute` call — not a human reading the tail of a log — is
  what decides whether an ambiguous-looking death was actually a complete
  run.

## Amendment rule

Any amendment to this pre-registration after this commit is a **separate
dated file** in `docs/superpowers/evidence/`, cross-linked from here by a
later commit, and **never** an in-place edit of this document — identical
in force to `docs/gates.md`'s house rule and spec §6's own non-silent
amendment rule. No endpoint, formula, seed, corpus byte, boot config field,
or digest pin changes after a gate number has been seen. The corpus is
bytes after this commit (§3.2 above); nothing in
`tools/memory_battery/corpus-v1/` is ever edited in place.

## Committed artifacts

- This document, `2026-08-26-memory-battery-preregistration.md`.
- `tools/memory_battery/corpus-v1/` — `manifest.json` + 50 ×
  `tasks/<name>/{workspace,pristine}/` — the frozen instrument, freeze sha
  `d9df82e2f7ae95130fc8fa765b5b1faff7b15e93832f8adfd1980b07d797c9d5` (§3.2).
- `tools/memory_battery/{corpus.py, corpus_check.py, driver.py,
  recompute.py, recompute_bootstrap.py, recompute_join.py,
  recompute_journal.py, __init__.py}` — already committed at `99bc54b`
  (Tasks 1–4), file shas pinned in §5, untouched by this commit.
