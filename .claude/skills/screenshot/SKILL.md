---
name: screenshot
description: Capture the running app as a PNG to review a UI change, on Linux (Claude Code on the web) through the GTK build under Xvfb. Use when asked for a screenshot, to check how a change looks, or after a change to what the shared UI shows. Also says what to do about docs/screenshot.png, the README image.
---

# Screenshots

The UI is written once (`wtm-ui`) and shown by a toolkit per OS. In a Linux
container the GTK build is the one that runs, and it can photograph itself:
a change to what the shared UI shows can be checked here, and the picture
shown to the user.

`docs/screenshot.png`, the README image, is a capture of the macOS app. It
cannot be made here: ask the user to retake it on their Mac when a change
should be reflected in it, and never replace it with a GTK capture.

## Capturing the GTK build

Never point the app at the real config. Everything below lives in a
throwaway directory: demo repos, the config, the window state, and a fake
`HOME`.

1. Make the sandbox: a repo or two with the states worth seeing (a linked
   worktree, a dirty file, a `claude/` or `cursor/` branch for the agent
   marks), and a `config.json` naming them:

   ```sh
   S=$(mktemp -d); mkdir -p "$S/home" "$S/data"
   git init -q -b main "$S/app" && git -C "$S/app" -c user.email=a@b -c user.name=a commit -q --allow-empty -m init
   git -C "$S/app" worktree add -q -b claude/fix-login "$S/app-wt"
   cat > "$S/data/config.json" <<EOF
   {"repos":[{"id":"r1","name":"app","path":"$S/app","mainBranch":"main","initCommand":""}],
    "worktreesRoot":"$S/wt","editorCommand":"true"}
   EOF
   ```

2. Build and capture (`scripts/cloud-setup.sh` has installed GTK and Xvfb):

   ```sh
   cargo build -p wtm-app
   HOME="$S/home" WTM_USER_DATA="$S/data" WTM_NO_LAUNCH=1 \
     WTM_SCREENSHOT="$S/shot.png" WTM_SCREENSHOT_QUIT=1 \
     xvfb-run -a -s "-screen 0 1600x1100x24" target/debug/worktree-manager
   ```

   The PNG is written once the first listing has settled.
   `WTM_APPEARANCE=dark` gives the dark look.

3. To photograph a dialog, the picker or Settings, drive the UI first with
   `WTM_SCREENSHOT_STEPS`, a comma-separated list run through the view's own
   handlers: `new-worktree`, `repo-settings`, `picker`, `settings`,
   `collapse:<card>`, `select:<row>`, `query:<text>`, `type:<text>`. Row and
   card keys are `r:<repo id>`, `w:<path>`. See `crates/wtm-gtk/src/screenshot.rs`.

4. Look at the image (Read the PNG) before saying anything about it, then
   put it under `/mnt/project-files/` to show the user, and delete the
   sandbox.

Fonts and widgets are GTK's (Adwaita, DejaVu), so the picture shows layout
and content, not how the Mac looks. Say so when handing it over.
