# Signed and notarized Mac builds

With these secrets set, every tagged release builds a Mac app signed with Kairo's Developer ID and
notarized by Apple: it opens with a double-click, no right-click → Open and no "unidentified
developer" warning. Without them, CI builds the ad-hoc signed app as before
(`.github/workflows/build.yml`, step "Developer ID signing (when set up)").

What gets signed: Tauri signs `Kairo.app` and `Syphon.framework` and sends the app to Apple's
notary service; the CI step signs the two programs Kairo runs from `Resources/bin` (`ffmpeg`,
`kairo-coreml-speech`) first, with the hardened runtime notarization requires.

## One-time setup

### 1. Developer ID Application certificate

1. Open **Keychain Access** → menu **Keychain Access → Certificate Assistant → Request a Certificate
   From a Certificate Authority…**. Enter your email and name, choose **Saved to disk**, save the
   `.certSigningRequest` file.
2. Go to <https://developer.apple.com/account/resources/certificates/add>, choose
   **Developer ID Application**, upload that file, and download the certificate.
3. Double-click the downloaded `.cer`: it joins your login keychain, next to the private key the
   request made.
4. In Keychain Access → **My Certificates**, right-click **Developer ID Application: … (TEAMID)** →
   **Export…** → save as `kairo-developer-id.p12` on the Desktop, with a password you choose.

The name in brackets is your Team ID. `security find-identity -v -p codesigning` prints the full
name, which is the signing identity below.

### 2. App-specific password (for notarization)

<https://account.apple.com> → **Sign-In and Security → App-Specific Passwords** → **+** → name it
"Kairo notarization" and copy the password it shows.

### 3. The GitHub secrets

Run each in Terminal from the project folder. A command without a value asks you to paste it.

```bash
base64 -i ~/Desktop/kairo-developer-id.p12 | gh secret set APPLE_CERTIFICATE -R A-Miide/Kairo
gh secret set APPLE_CERTIFICATE_PASSWORD -R A-Miide/Kairo
gh secret set APPLE_SIGNING_IDENTITY -R A-Miide/Kairo
gh secret set APPLE_ID -R A-Miide/Kairo
gh secret set APPLE_PASSWORD -R A-Miide/Kairo
gh secret set APPLE_TEAM_ID -R A-Miide/Kairo
```

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | the exported `.p12`, base64 (the first command does this) |
| `APPLE_CERTIFICATE_PASSWORD` | the password chosen when exporting it |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | the Apple ID email of the developer account |
| `APPLE_PASSWORD` | the app-specific password from step 2 |
| `APPLE_TEAM_ID` | the 10-character Team ID |

Afterwards, delete `kairo-developer-id.p12` from the Desktop (the certificate stays in your keychain).

## Checking a build

```bash
codesign --verify --deep --strict --verbose=2 /Applications/Kairo.app
spctl --assess --type execute --verbose /Applications/Kairo.app
```

`spctl` should say `accepted` and `source=Notarized Developer ID`.

The first signed version asks for microphone and camera access once more: macOS ties those
permissions to how an app is signed.
