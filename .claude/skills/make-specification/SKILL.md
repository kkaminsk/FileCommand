---
name: make-specification
description: Build a new OpenSpec proposal from a prompt and/or reference files, verify it, and create the matching Linear tracking issue. Use when the user wants to turn an idea, request, or set of notes/docs into a formal FileCommand specification.
allowed-tools: Bash(openspec:*), Bash(git status), Bash(git rev-parse:*), Bash(git branch:*), Bash(git checkout:*), Bash(git add:*), Bash(git commit:*), mcp__plugin_linear_linear__save_issue, mcp__plugin_linear_linear__list_projects, mcp__plugin_linear_linear__list_users, mcp__plugin_linear_linear__list_issue_labels, mcp__plugin_linear_linear__list_issue_statuses
metadata:
  author: filecommand
  version: "1.0"
---

Build a new OpenSpec proposal end-to-end: verify the right model and branch are in use, draft the proposal in Plan Mode, materialize it with the OpenSpec CLI, verify it for structural and factual accuracy, then create a Linear tracking issue for it.

---

**Input**: Zero or more file paths (design notes, research docs, images, prior specs) and/or free-form prose describing the desired change, in any combination. If nothing usable is given, ask the user what to build.

**Steps**

1. **Verify Fable is the active model**

   State which model is currently active (from your own session context). The last three OpenSpec proposals in this repo (`enter-file-action-menu`, `visual-themes`, `responsive-layout`) were all authored with **Claude Fable 5** as commit co-author — that is this project's established convention for spec-writing.

   If the active model is not Fable 5 (`claude-fable-5`), **stop here**. Tell the user:
   > "This project authors OpenSpec proposals with Claude Fable 5. The active model is `<current model>`. Run `/model` to switch to Fable 5, then re-invoke this skill."

   Do not proceed to any other step until running as Fable 5.

2. **Verify the branch**

   ```bash
   git rev-parse --abbrev-ref HEAD
   ```

   Per `CLAUDE.md`, OpenSpec proposals must be drafted on the `Spec` branch (never on `main`, and not on a `build/*` branch). If the current branch is not `Spec`:

   - Run `git status` first.
   - If the working tree is dirty, **stop** and ask the user to commit or stash (`git stash -u`) their in-progress work before switching — never switch over uncommitted changes.
   - If the tree is clean, ask the user to confirm switching to `Spec` (`git checkout Spec`). Only run the checkout after they confirm.

   Do not proceed to drafting until HEAD is on `Spec`.

3. **Gather inputs and prior art**

   - Read every file the user provided.
   - Run `openspec list` (and skim `openspec/specs/`) to check for existing capabilities or in-flight changes under `openspec/changes/` that overlap with the request — avoid duplicating or contradicting them.
   - Only skim `docs/superpowers/specs/2026-08-06-filecommand-design.md` for cited rationale; it is frozen history and must never be edited.

4. **Draft the proposal in Plan Mode**

   Call the **EnterPlanMode tool** if not already in plan mode. While planning:
   - Use the **AskUserQuestion tool** to resolve any ambiguity in the request (scope, which capabilities are touched, breaking vs. non-breaking, etc.).
   - Derive a kebab-case change name.
   - Draft the full content that will become `proposal.md` (Why / What Changes / Capabilities New+Modified / Impact), `design.md` (Context / Goals-NonGoals / Decisions / Risks-Tradeoffs / Open Questions), `tasks.md` (checklist tied to spec requirements), and the spec delta(s) for each touched capability (ADDED/MODIFIED/RENAMED/REMOVED Requirements with Given/When/Then Scenarios).
   - Write this drafted content into the plan file.

   Call the **ExitPlanMode tool** to get the user's approval. Do **not** write any real files under `openspec/` before approval.

5. **Materialize the proposal via the OpenSpec CLI**

   Once the plan is approved, follow the exact mechanics from `.claude/skills/openspec-propose/SKILL.md`:

   ```bash
   openspec new change "<name>"
   ```

   Then loop:
   ```bash
   openspec status --change "<name>" --json
   openspec instructions <artifact-id> --change "<name>" --json
   ```
   For each `ready` artifact, write the approved content (from step 4) to the `resolvedOutputPath` the CLI returns — never hardcode paths. Repeat until every artifact in `applyRequires` is `done`.

6. **Verify accuracy**

   - Structural check:
     ```bash
     openspec validate <name> --strict --json
     ```
     Fix any reported issues and re-validate until clean.
   - Factual check: review (or spawn a review agent for) every claim in `proposal.md`, `design.md`, and the spec deltas against the original inputs (files/prompt, plus any cited design-doc sections). Flag and fix anything unsupported or contradicted by the source material — do not let an inaccurate proposal reach the user as "done."

7. **Offer to commit — do not commit automatically**

   Show the user the new/changed files (`git status`). Ask whether to commit now. Only if they confirm:
   ```bash
   git add openspec/changes/<name>
   git commit -m "Add OpenSpec proposal <name>

   <one-paragraph summary>

   Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
   ```
   Never push, never open a PR, and never touch `main` from this skill — per `CLAUDE.md`, reaching `main` requires a separate, explicitly user-confirmed PR.

8. **Update Linear**

   Create exactly **one** issue for this proposal (not one per capability or per task):
   - Resolve ids live — never hardcode them from memory:
     ```
     list_projects (query: "File Command")
     list_users (query: "Kevin Kaminski")
     list_issue_labels
     list_issue_statuses (team: "BigHatGroup")
     ```
   - `save_issue` with:
     - Project: File Command
     - Title: the proposal's one-line summary
     - Description: the Why + What Changes bullets from `proposal.md`, plus a reference to `openspec/changes/<name>/proposal.md`
     - Assignee: Kevin Kaminski
     - Status: Backlog
     - Label: best fit of Feature / Improvement / Bug inferred from the proposal content (default Feature)
   - Report the created issue's URL back to the user.

**Output**

Summarize: model and branch checks passed, proposal name and files written, validation result, whether the proposal was committed, and the Linear issue URL. Remind the user of the remaining manual steps per `CLAUDE.md`: open a PR from `Spec` into `main`, get it explicitly confirmed and merged, then a `build/<batch-name>` branch off `main` can implement it.

**Guardrails**

- Never proceed past step 1 or 2 unless the stated condition is actually true, or the user explicitly overrides with a direct instruction.
- Never write files under `openspec/changes/` before `ExitPlanMode` approval.
- Never commit, push, or touch `main` without explicit per-action user confirmation, even though this skill operates on `Spec`.
- Never fabricate Linear project/user/label/status ids — always resolve them via a tool call first.
- If a change with the derived name already exists, ask whether to continue it or pick a new name.
