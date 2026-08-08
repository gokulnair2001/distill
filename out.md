---
title: Crisper — Speak. Get perfect text.
author: Crisper
lang: en
site: Crisper
---

macOS · On-Device AI · 100% Private

# Speak.
 *Get perfect text.*

Crisper turns your voice into polished, ready-to-paste text — instantly, privately, and entirely on your Mac. No cloud. No subscriptions. Just speak.

[Download for Mac](https://github.com/gokulnair2001/crisper-releases/releases/latest/download/Crisper.dmg) [See features](#features)

Requires macOS 14 Sonoma · Apple Silicon recommended

100% on-device

No account

No subscription

![Crisper app icon](https://www.speakcrisper.com/Assets/Icon.png)

Idle

Last transcription

**I think we should restructure the API** to support batch operations — it'll make the mobile client much faster.

---

Features

## Built for how you think.

No friction. No setup. A tool that gets out of the way and lets you speak.

01

### Floating pill, always ready

A minimal capsule that floats above every window. No dock icon, no menubar clutter — just a single hotkey away whenever you need it.

02

### Three recording modes

Toggle, hold-to-record, or re-paste last. Fully rebindable — tune it to how your brain works.

⌥Space Toggle recording

⌥R Hold to record

⌥V Re-paste last

03

### Auto-paste, zero interruption

Crisper captures which app you were in before recording. When done, it pastes directly back — Slack, Notion, VS Code, anywhere.

04

### Full transcript library

Every recording saved with its source app, timestamp, and word count. Search, filter, pin, replay — your personal voice archive.

Sprint planning thoughts

Notion · 2:45 PM

Draft email to design team

Slack · 11:20 AM

Code review comments

VS Code · Yesterday

05

### Two-stage AI polish

Crisper's speech model transcribes with unmatched accuracy. A local Crisper model then fixes grammar, removes filler words, and makes it sound intentional.

06

### Audio playback with scrubbing

Replay any recording with an interactive waveform. Scrub to any point, compare what you said to what was transcribed.

07

### Fully customizable hotkeys

Visual key-capture UI. Record any combination, changes take effect immediately. No app restart required.

---

How it works

## From thought to text *in seconds.*

The entire pipeline runs on your Mac. Fast, private, and invisible.

01 — Speak

#### Press and speak

Hit ⌥Space or hold ⌥R. The waveform confirms you're being heard. Say anything — notes, emails, code comments.

02 — Transcribe

#### On-device transcription

Crisper's speech model runs entirely on your Mac via CoreML, converting audio to raw text with state-of-the-art accuracy.

Transcribing…

03 — Polish

#### AI refines the text

Crisper's on-device language model cleans up the raw transcript — grammar, filler words, repetitions — while preserving your voice.

"um so I was thinking maybe we could uh restructure the api"

↓

"I think we should restructure the API."

04 — Paste

#### Back in your app

The polished text is automatically pasted into whatever you were working in — Slack, Notion, Mail, VS Code. Done.

Pasted

---

Privacy

## Your voice never leaves *your Mac.*

Crisper runs entirely on-device using open-source AI models. No audio uploaded. No text sent anywhere. No account required.

Audio processed entirely on your device

Text polish via Crisper's language model — fully local, powered by OSS

Transcripts stored in `~/Library/Application Support/Crisper/`

No telemetry, no analytics, no cloud sync

Works fully offline after first-run model download

🎙️

Microphone

AVFoundation · local audio capture

↓

🧠

Crisper Speech Model

OSS · CoreML · on-device

↓

✦

Crisper Language Model

OSS · on-device · fully local

↓

📋

Polished text — pasted

NSPasteboard · Accessibility API

Zero cloud. Zero leaks. Zero compromises.

---

Compatibility

## Paste into any app.

Crisper tracks which app you were in and pastes right back. Source app is also saved with the transcript for context.

Notion Slack VS Code Notes Mail Linear Figma Zoom Arc Chrome Safari Messages Terminal + any app

---
