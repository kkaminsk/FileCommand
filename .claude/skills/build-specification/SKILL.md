---
name: build-specification
description: Implement OpenSpec proposals that have an open Linear tracking issue in the File Command project, using multi-agent (ultracode/Workflow) orchestration on per-proposal build/<name> branches off main. Closes issues that finish successfully; leaves blocked ones open with a status comment. Use when the user wants to build/implement pending specifications, work through open Linear issues for FileCommand, or run "build specification" / "build all".
allowed-tools: Bash(openspec:*), Bash(git status), Bash(git rev-parse:*), Bash(git branch:*), Bash(git checkout:*), Bash(git merge-base:*), Bash(git cat-file:*), Bash(git add:*), Bash(git commit:*), Bash(cargo build *), Bash(cargo test *), mcp__plugin_linear_linear__list_issues, mcp__plugin_linear_linear__get_issue, mcp__plugin_linear_linear__save_issue, mcp__plugin_linear_linear__save_comment, mcp__plugin_linear_linear__save_status_update, mcp__plugin_linear_linear__list_issue_statuses
metadata:
  author: filecommand
  version: "1.0"
---

Implement OpenSpec proposals tracked by open Linear issues in the File Command project — gate each against git reality, implement via multi-agent orchestration (ultracode), verify with a real build/test pass, and keep Linear in sync: close what finished, leave a status comment on what's blocked.

---

**Input**: Optional proposal/issue names or identifiers (e.g. `visual-themes` or `BIG-12`) to restrict the run. **No input defaults to "build specification all"** — process every open, OpenSpec-linked issue in the File Command project.

**Steps**

1. **Discover candidates**

   ```
   list_issues (project: "File Command", includeArchived: false)
   ```

   Keep issues whose `statusType` is not `completed`/`canceled` and whose title carries the `(OpenSpec: <name>)` tag (the established convention, e.g. `BIG-11`/`BIG-12`) — extract `<name>` from the tag. If the user named specific proposals/issues, filter down to those.

   Also run `openspec list` and diff against the candidate set: any proposal under `openspec/changes/` with **no** matching Linear issue is out of scope for this skill (that's what `make-specification` handles) — just note it in the final output, don't act on it.

2. **Gate each candidate against git, never against Linear's own text**

   For each candidate's proposal name, check whether it actually exists on `main`:
   ```bash
   git cat-file -e main:openspec/changes/<name>/proposal.md
   ```
   If this fails (not on `main` yet), the proposal still needs its `Spec` → `main` PR merged — a manual step outside this skill's authority:
   - Do not create/touch its `build/<name>` branch or any code.
   - `save_comment` on the issue stating plainly what's blocking it (e.g. "proposal not yet merged to main — needs a user-confirmed PR from Spec before implementation can start") and correct the issue's own text if it claims otherwise or if it's already stale (Linear descriptions can lag git — verify, don't trust).
   - Leave the issue's status untouched. Move on to the next candidate.

   If it succeeds, the proposal is ready to implement.

3. **Prepare the branch**

   ```bash
   git status
   git branch --list build/<name>
   ```
   - If `build/<name>` already exists, check it out and keep whatever is there — it may be genuine in-progress work (uncommitted WIP on an existing build branch has been produced by prior sessions before; never discard it).
   - If it doesn't exist, `git checkout -b build/<name> main`.
   - Never switch branches over uncommitted changes that belong to different, unrelated work without asking the user first.

4. **Verify Sonnet is the active model before using ultracode**

   State which model is currently active (from your own session context). Implementation work in this project is done with **Claude Sonnet 5** (`claude-sonnet-5`) — this is a distinct convention from `make-specification`'s spec-writing check, which requires Fable instead.

   If the active model is not Sonnet 5, **stop here, before invoking the Workflow tool or touching any branch**. Tell the user:
   > "This skill implements proposals with Claude Sonnet 5. The active model is `<current model>`. Run `/model` to switch to Sonnet 5, then re-invoke this skill."

   Do not proceed to step 5 until running as Sonnet 5.

5. **Implement every ready proposal via the Workflow tool (ultracode)**

   Use the **Workflow tool** to implement the ready proposals — this is the required "worked on using ultracode" step. Pipeline across proposals so one doesn't block another. For each proposal, the workflow agent(s) should:
   - Follow `.claude/skills/openspec-apply-change/SKILL.md`'s exact mechanics: `openspec instructions apply --change "<name>" --json` for the pending-task list, implement each task, and flip `- [ ]` → `- [x]` in `tasks.md` only once a task is genuinely verified done.
   - Reconcile against any pre-existing uncommitted work already on the branch rather than assuming `tasks.md` checkboxes reflect reality — checkboxes can lag real code (verify via build/tests, read the diff against the proposal's Impact section, don't blindly re-implement or blindly trust "looks done").
   - Stop implementing *that specific proposal* (others in the run continue independently) if a task needs a manual/human step: an ambiguous product decision, a credential or external resource, or any action this assistant can't take unilaterally (destructive, irreversible, or `main`-touching). Record exactly what's blocking it for step 6.

   After each proposal's tasks are all checked, run:
   ```bash
   cargo build --workspace
   cargo test --workspace
   ```
   on that branch. If either fails, the proposal is **not** complete — treat it like a mid-implementation block and record what failed.

6. **Commit finished work — never push, never touch `main`**

   Once a proposal's `tasks.md` is fully checked and build+tests are green, review `git status` and stage **only files plausibly belonging to that change** (cross-check against the proposal's own Impact section, e.g. the `crates/**` paths it names). Flag anything unexpected — stray top-level files, unrelated directories — to the user instead of silently including it. Commit on the `build/<name>` branch:
   ```bash
   git commit -m "Implement OpenSpec change <name>

   <one-paragraph summary of what was implemented>"
   ```
   Never `git push`, never open a PR, never touch `main` — per `CLAUDE.md`, those require a separate, explicit user go-ahead.

7. **Update Linear per proposal**

   - **Completed** (tasks done, build+tests green, committed): move the issue to **Done** and `save_comment` summarizing what shipped, which branch it's on, and that it's ready for a user-confirmed PR into `main` whenever wanted.
   - **Blocked mid-implementation** (step 5): leave the issue open (state unchanged, or **In Progress** if real work landed) and `save_comment` with current progress plus exactly what manual step is needed to continue.
   - **Blocked before starting** (step 2's gate): handled as described there.

8. **Post a final Linear status update**

   After the whole run, once via:
   ```
   save_status_update (type: "project", project: "File Command")
   ```
   summarizing: which issues closed, which are still open and why, and the `build/<name>` branches involved.

**Output**

A per-proposal summary: gate result, implementation outcome, the Linear issue's new state, and its branch — matching what was posted in the final project status update. Call out any proposal that had no matching Linear issue at all.

**Guardrails**

- Never invoke the Workflow tool (ultracode) unless the active model is verified as Sonnet 5 first.
- Never trust a Linear issue's own description over what git actually shows for "is this proposal merged to main."
- Never discard or overwrite uncommitted work on any branch without the user's confirmation.
- Never mark a task `[x]` or an issue Done without an actual passing `cargo build`/`cargo test` on that branch.
- Never push, open a PR, or touch `main` — report readiness and wait to be asked.
- Never silently stage files outside the proposal's stated Impact area — ask about anything unexpected.
- One proposal's manual-step block must never stop the rest of the batch from being processed.
