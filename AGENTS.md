# Project conventions

- Answer users in simple, direct Simplified Chinese, explaining the specific changes and why they were made.
- If a user instruction is unclear, or the actual implementation differs from the user's description, ask the user before proceeding.
- If there are multiple possible interpretations, point them out explicitly; do not assume or choose one on your own.
- If there is a better or simpler approach, propose it to the user. You may challenge an unreasonable approach proposed by the user.
- If anything becomes unclear while writing code, stop immediately and ask the user.
- Keep everything simple. Write only the minimum code needed to solve the problem; do not add speculative extensions or provisions for future use unless the user emphasizes them.
- Do not add content the user did not request.
- Do not create abstractions for code used only once.
- Do not write error handling for impossible cases.
- When reviewing code, if a section can be implemented more simply, change it directly.
- Each inline comment block may contain at most two lines. If more are needed, the code has a patchwork design problem that should be corrected; do not use long comments to explain what such code does.
- Comments should explain why the code was written rather than repeat what it does. (Documentation comments for exported APIs, such as `@param`, `@returns`, and `@deprecated`, are documentation and are not subject to this restriction.)
- Make targeted changes to the code that needs them; do not refactor code without a problem.
- Follow the project's existing code style, even if it is unconventional.
- Every code change must correspond to a user requirement.
- Before writing code, turn the task into verifiable goals. For example, “fix a bug” means “write a test that reproduces the bug and make it pass”; “refactor” means “ensure the tests pass both before and after the refactor.”
- For complex tasks, use several short plans and state how each step can be verified.
- When running Linux-related tests on Windows, use local WSL and test both Windows and Linux functionality. In a Linux environment, use the current environment. In a Mac environment, run only Mac-related tests.
- Refer to `docs/UI-requirements.md` when writing UI-related code.
- Keep the project structure clear. Create files or crates when needed rather than mixing code responsible for different modules.
- Read the local README. If a directory to be modified contains a README, also check its parent directories for READMEs and read them before making changes.
- Fix upstream problems rather than patching them downstream. If a new feature exposes a limitation in an existing module, identify the upstream module responsible and propose a change to the user. Unless the user asks otherwise, do not implement the feature solely by adding a downstream patch.
- Prefer existing libraries. Read the documentation or inspect the library to check for built-in options, existing solutions, or examples. Write custom code only when no sufficiently suitable alternative exists.
- Promptly clean up temporary files used during development, such as temporary validation scripts and generated documents.

# Git

- This is a personal project, so a new branch is not required for every change unless there is a special requirement or the user requests one.
- Before the primary agent starts a modification task, check the branch and `git status --porcelain=v1`. There must be no staged, unstaged, or untracked files from before the task; otherwise, stop making changes and report the situation. Read-only analysis and review may continue. Changes produced by the current task do not trigger this stop condition.
- Do not clean up, stage, discard, overwrite, or commit the user's existing changes, except files produced by the current task.
- Commit all code, configuration, and documentation changes produced by each task, except anything the user explicitly asks not to commit. Stage only the files you are responsible for; inspect the staged diff before committing to ensure no other task's changes are included.
- Each commit must contain one coherent change, use a prefix such as `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, or `chore:`, and describe the specific change. If the user requests several changes, make several small commits, each with a single purpose; do not combine many unrelated changes into one commit.
- At the end of the task, push only the master branch to the remote unless the user requests pushing a specific branch.
# Documentation
- Every crate must have its own README explaining its purpose. Update the README whenever the corresponding code changes so that the documentation matches the current code.
- Promptly summarize project-wide features in the root README, but keep the main README concise. Avoid excessive technical detail or long passages that discourage reading; ideally, combine text with illustrations.
- When releasing a major version, record all additions and changes since the previous release in the changelog.

# Internationalization
- All text displayed in the application must use i18n and support characters of different widths. Do not hardcode it in the program (except for certain emergency uses).