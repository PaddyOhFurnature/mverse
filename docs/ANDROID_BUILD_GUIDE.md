# Android Relay - Rust + NDK Build Guide

## Step 1: Install Android NDK

**Option A: Via Android Studio (Recommended)**
1. Install Android Studio: https://developer.android.com/studio
2. Open Android Studio → Tools → SDK Manager
3. SDK Tools tab → Check "NDK (Side by side)" → Apply
4. NDK installs to: `~/Android/Sdk/ndk/<version>/`

**Option B: Command line (Faster)**
```bash
# Download NDK directly
cd ~
wget https://dl.google.com/android/repository/android-ndk-r26d-linux.zip
unzip android-ndk-r26d-linux.zip
export ANDROID_NDK_ROOT=~/android-ndk-r26d

# Add to ~/.bashrc for persistence:
echo 'export ANDROID_NDK_ROOT=~/android-ndk-r26d' >> ~/.bashrc
echo 'export PATH=$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH' >> ~/.bashrc
```

## Step 2: Install cargo-ndk (Build Helper)

```bash
cargo install cargo-ndk
```

This wraps cargo to use NDK's clang instead of gcc.

## Step 3: Project Structure

We'll create:
```
android-relay/
├── rust/                     # Our Rust relay (as library)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs           # JNI bindings
│       └── relay.rs         # Our metaverse_relay code
├── android/                  # Android Studio project
│   ├── app/
│   │   ├── src/main/
│   │   │   ├── java/
│   │   │   │   └── com/metaverse/relay/
│   │   │   │       ├── RelayService.java
│   │   │   │       └── MainActivity.java
│   │   │   └── jniLibs/     # Built .so files go here
│   │   │       ├── arm64-v8a/
│   │   │       ├── armeabi-v7a/
│   │   │       ├── x86/
│   │   │       └── x86_64/
│   │   └── build.gradle
│   └── build.gradle
└── build.sh                  # Build script
```

## Step 4: Configure Rust Library

**Cargo.toml changes:**
```toml
[lib]
name = "metaverse_relay"
crate-type = ["cdylib"]  # Creates .so for Android

[dependencies]
# Add JNI support
jni = "0.21"
# ... existing dependencies
```

## Step 5: Create JNI Bindings

**src/lib.rs:**
```rust
use jni::JNIEnv;
use jni::objects::JClass;
use jni::sys::jstring;

mod relay;  // Our relay code

#[no_mangle]
pub extern "C" fn Java_com_metaverse_relay_RelayService_startRelay(
    env: JNIEnv,
    _class: JClass,
    port: i32,
) -> jstring {
    // Start relay, return peer ID
    let peer_id = relay::start(port as u16);
    let output = env.new_string(peer_id).unwrap();
    output.into_raw()
}

#[no_mangle]
pub extern "C" fn Java_com_metaverse_relay_RelayService_stopRelay(
    _env: JNIEnv,
    _class: JClass,
) {
    relay::stop();
}
```

## Step 6: Build Script

**build.sh:**
```bash
#!/bin/bash
# Build for all Android architectures

export ANDROID_NDK_ROOT=~/android-ndk-r26d

# Build for each architecture
cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release
cargo ndk -t armeabi-v7a -o android/app/src/main/jniLibs build --release
cargo ndk -t x86 -o android/app/src/main/jniLibs build --release
cargo ndk -t x86_64 -o android/app/src/main/jniLibs build --release

echo "✅ Native libraries built!"
echo "Now build APK in Android Studio"
```

## Step 7: Android Service

**RelayService.java:**
```java
package com.metaverse.relay;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;

public class RelayService extends Service {
    static {
        System.loadLibrary("metaverse_relay");
    }
    
    private native String startRelay(int port);
    private native void stopRelay();
    
    private String peerId;
    
    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        int port = intent.getIntExtra("port", 4001);
        peerId = startRelay(port);
        
        // Show notification with peer ID
        showNotification("Relay running", "Peer ID: " + peerId);
        
        return START_STICKY;  // Restart if killed
    }
    
    @Override
    public void onDestroy() {
        stopRelay();
        super.onDestroy();
    }
    
    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
```

## Next Steps

Ready to start? I'll:

1. ✅ Rust targets installed
2. ⏳ Need you to install NDK (Option A or B above)
3. ⏳ Create the project structure
4. ⏳ Set up Cargo for Android builds
5. ⏳ Write JNI bindings
6. ⏳ Create Android project
7. ⏳ Build APK

**Which NDK install method do you prefer?**
- Option A: Android Studio (bigger download, GUI)
- Option B: Direct download (faster, command line)
