# Security Policy

Asset Tap is a small open-source project. We take security seriously and appreciate
reports that help keep users safe.

## Supported Versions

Asset Tap is a rolling-release desktop app versioned with CalVer. Only the
[latest release](https://github.com/nightandwknd/asset-tap/releases/latest) is supported.
Please upgrade before reporting a suspected vulnerability, in case it is already fixed.

## Reporting a Vulnerability

Please report vulnerabilities **privately** — do not open a public issue for security
problems.

Use GitHub's private vulnerability reporting for this repository:

1. Go to the [Security tab](https://github.com/nightandwknd/asset-tap/security).
2. Click **Report a vulnerability** to open a private advisory.

Include as much as you can: affected version, platform (macOS/Linux/Windows), whether it
involves the GUI or CLI, reproduction steps, and impact.

## What to Expect

This is a small project maintained on a best-effort basis. We aim to acknowledge reports
within a week and to keep you updated as we investigate. We are not able to offer bug
bounties, but we will credit reporters in the release notes if you'd like.

## Scope Notes

The most security-relevant surfaces of Asset Tap are:

- **Local API-key storage** — provider API keys are stored on the local machine
  (settings/config directory). Keys are used to authenticate outbound requests to the
  configured AI providers (e.g. fal.ai, Meshy).
- **Bundle import & demo download** — Asset Tap extracts `.zip` archives when importing
  bundles or downloading the demo bundle. Report anything that could let a crafted archive
  write outside the intended directory or otherwise misbehave.
- **Blender invocation** — the optional FBX export stage shells out to Blender. Report any
  path or argument handling that could lead to unintended command execution.
- **Provider/template YAML** — providers and templates are data-driven YAML configs loaded
  from disk. Report parsing behavior that could be abused.

Issues in third-party dependencies are tracked with `cargo audit` (run in CI); if you find
one that affects Asset Tap in a way the advisory database doesn't yet cover, please let us
know through the private channel above.
