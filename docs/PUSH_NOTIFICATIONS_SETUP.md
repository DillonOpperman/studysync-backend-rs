# Push notifications setup (Phase 8G.2)

StudySync stores alerts in PostgreSQL. Firebase Cloud Messaging (FCM) is only a delivery channel. The in-app Alerts center remains the source of truth.

Delivery is **at-least-once**. Each in-app notification inserts at most one `push_outbox` row (`UNIQUE (notification_id)`). A background worker claims due rows with `FOR UPDATE SKIP LOCKED`, sends to every active device for that user, and retries transient FCM failures with bounded backoff (max 8 attempts). A retry can therefore deliver more than once to the same device. The client ignores duplicate taps on the same notification id.

## 1. What works without Firebase

With `PUSH_ENABLED` unset or false, or with missing `FCM_PROJECT_ID` / `GOOGLE_APPLICATION_CREDENTIALS`:

- The Rust API starts normally.
- Auth, groups, chat, media, and `GET /api/v1/notifications` keep working.
- Creating an event still writes the in-app notification and an outbox row.
- The worker does not start, and the process makes **no** Google HTTP calls.
- `POST /api/v1/push/devices` still registers devices (so the app can attach a token later).
- The Android app builds and launches without `google-services.json`. Push stays in development mode.

You do **not** need a Firebase account, billing, Apple Developer membership, or Google Play to develop locally.

## 2. Create a free Firebase project

1. Open [Firebase Console](https://console.firebase.google.com/) with a Google account.
2. Create a project. You can skip Google Analytics.
3. Stay on the Spark (free) plan. Do not enable Blaze/billing for this phase.
4. Do **not** turn on Firebase Authentication, Firestore, or Realtime Database. StudySync does not use them.

## 3. Register the Android application

1. In Project settings, add an Android app.
2. Use package name `com.studysync.app` (see `android/app/build.gradle`).
3. SHA-1 is optional for FCM token delivery; add a debug SHA-1 later if Google Play services requires it.

## 4. Obtain `google-services.json`

Download Firebase’s Android config file into:

`studysync-frontend/android/app/google-services.json`

That file is **public client configuration** (app id, project number). It is still machine-specific: **do not commit it**. The Gradle Google Services plugin is applied only when this file exists.

There is no dummy `google-services.json` in the repo. Do not invent project ids.

## 5. Configure the Rust FCM service account

1. In Firebase / Google Cloud, create a service account with permission to send FCM (Firebase Cloud Messaging API enabled; role such as Firebase Cloud Messaging Admin).
2. Create a JSON key on your machine. This file is a **private credential**. Never commit it, never put it in FCM payloads, never paste it into chat logs.
3. Point the API at it with `GOOGLE_APPLICATION_CREDENTIALS` (absolute path).
4. Set `FCM_PROJECT_ID` to the Firebase/Google Cloud project id.

The backend uses FCM **HTTP v1** and a short-lived OAuth 2.0 access token. It does not use the legacy server-key API.

## 6. Enable push delivery locally

In `studysync-backend-rs/.env`:

```
PUSH_ENABLED=true
FCM_PROJECT_ID=your-gcp-project-id
GOOGLE_APPLICATION_CREDENTIALS=C:\path\to\service-account.json
```

Run migrations (`sqlx migrate run`) so `0013_push.sql` is applied, then `cargo run`.

Startup logs report `push_requested` and `push_delivery_enabled` only. They do not print tokens, notification bodies, or the service-account key.

## 7. Test on Android

1. Place `google-services.json` as above.
2. Rebuild the Android app (`assembleDebug` / `npx react-native run-android`).
3. Sign in. Grant notification permission on Android 13+.
4. Confirm `POST /api/v1/push/devices` succeeds (device id, no token in the JSON).
5. Trigger a real event (friend request, DM, announcement).
6. Confirm an Alerts row exists even if the OS banner does not.
7. With FCM configured, a backgrounded device should receive an OS notification whose visible copy is conservative (direct messages show “New message on StudySync”).
8. Tap it: session restore, then navigation via the existing Alerts destinations, or Alerts if the payload cannot be authorized.

This repository does not claim live FCM delivery until you have completed the steps above on a real device.

## 8. Disable push

Set `PUSH_ENABLED=false` or remove `GOOGLE_APPLICATION_CREDENTIALS`. Restart the API. In-app Alerts are unchanged. Outbox rows may remain `pending` until delivery is enabled again.

On the device, turn off the global push switch in Alerts → Notification settings, or revoke OS permission.

## 9. Rotate credentials

1. Issue a new service-account JSON key.
2. Point `GOOGLE_APPLICATION_CREDENTIALS` at the new file.
3. Restart the API.
4. Disable/delete the old key in Google Cloud.
5. FCM device tokens rotate independently; the app re-registers on refresh. The same token moving to another signed-in account revokes the previous owner’s row.

## 10. What remains for iOS

iOS types, permission helpers, and Info.plist `remote-notification` background mode are prepared. **APNs delivery is untested on Windows.** Remaining Mac/Xcode work:

1. Apple Developer membership (paid) for a real device and APNs.
2. App ID with Push Notifications capability.
3. `aps-environment` entitlement (development/production) signed with your Team ID. Do not fabricate Team IDs here.
4. APNs key or certificate uploaded to Firebase.
5. Add the iOS app in Firebase and place `GoogleService-Info.plist` locally (do not commit).
6. On macOS: `pod install`, enable Push in Xcode, run on a physical iPhone.
7. Confirm `UNUserNotificationCenter` permission, FCM/APNs token, foreground presentation, and cold-start taps.

`USE_FRAMEWORKS=static` is often required for React Native Firebase iOS; apply that only when building on a Mac.

## 11. Paid accounts for distribution

| Need | Paid? |
| --- | --- |
| Local Android + Rust development | No |
| Firebase Spark + FCM | No (stay on Spark) |
| Sideload a debug APK | No |
| Google Play listing | Yes (Play Console) |
| iOS device APNs / App Store | Yes (Apple Developer) |
| Production hosting | Separate from this phase |

## 12. Known limitations

- Ordinary group chat messages still do not create notifications (announcements only).
- Push previews are generic; Alerts still stores the fuller in-app copy.
- Outbox delivery is at-least-once, not exactly-once.
- Device tokens are stored for sending. They are never returned in DTOs or written to logs. There is no KMS envelope encryption in this phase.
- Without `google-services.json`, the Android app runs and shows a development-mode explanation instead of a fake “enabled” state.
