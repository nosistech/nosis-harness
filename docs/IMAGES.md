# Work from a screenshot

Version 0.2.2 enables image input on the `deepseek-v4-flash` route.
It sends the provider's current `deepseek-flash` model ID. The route name stays
the same so existing commands keep working. DeepSeek Pro remains text-only.
Older v0.2.1 binaries do not include this change.

Put the PNG or JPEG inside your project folder. Only attach images you intend
to share with the selected provider. Images contribute to the provider's token
usage and can incur charges.

## In a chat

Start a chat from your project folder:

```powershell
nh chat --model deepseek-v4-flash
```

At the Nosis prompt, attach the image:

```text
/image screenshot.png
```

Then type your task, for example: "Explain the error in this screenshot."
The image is sent with your next task. An attachment does not send a request by itself.
For a path containing spaces, type the full path after `/image` without quotation marks.

## In one command

```powershell
nh run "Explain the error in this screenshot" --model deepseek-v4-flash --image screenshot.png
```

Use the full path to `nh.exe` if your portable copy is not on PATH.

## Limits

- PNG and JPEG only; the file bytes must match the extension.
- At most four images per message, each at most 3.5 MiB.
- The image must pass the existing project path and read-approval checks.
- Provider dimension limits still apply. For DeepSeek, keep each side at or below
  8192 pixels. Resize an image if the provider rejects its dimensions.

Nosis does not capture your screen automatically. Other image-capable routes use
the same attachment commands. Select the model explicitly before attaching.
See [DeepSeek's vision documentation](https://api-docs.deepseek.com/guides/vision/)
for provider-specific limits.
