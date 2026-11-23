package dev.immineal.schemview;

import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.database.Cursor;
import android.graphics.Insets;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Process;
import android.window.OnBackInvokedDispatcher;
import android.provider.OpenableColumns;
import android.util.Log;
import android.view.DisplayCutout;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.MessageDigest;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

/**
 * Hosts the Rust viewer (libschemview_android.so, a NativeActivity) and does what needs the
 * Android framework: file pickers, "Open with" / "Share" intents, copying content:// files to
 * local storage, downloading the texture jar from Mojang, and full-screen window insets.
 * Results go to Rust through the native* methods (android/native/src/lib.rs).
 *
 * Rust calls pickSchematic(), pickJar(), downloadTextures() and goBack() over JNI, from its
 * own thread, so each of them hops to the UI or IO thread.
 */
public class MainActivity extends NativeActivity {
    private static final String TAG = "schemview";
    private static final int REQ_OPEN = 1;
    private static final int REQ_JAR = 2;
    private static final String MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

    /** One IO thread: copies and downloads run in order and never on the UI thread. */
    private final ExecutorService io = Executors.newSingleThreadExecutor();
    private volatile boolean downloading;
    /** The activity running the viewer; a second one in the same process only forwards its file. */
    private static MainActivity sLive;
    private boolean duplicate;

    static {
        // NativeActivity dlopens the library itself, which doesn't register it with the class
        // loader; this makes the Java_* functions below resolvable (same library, loaded once).
        System.loadLibrary("schemview_android");
    }

    static native void nativeOpen(String path);
    static native void nativeStatus(String text, boolean sticky);
    static native void nativeTexturesChanged();
    static native void nativeInsets(int left, int top, int right, int bottom);
    static native void nativeBack();

    // ------------------------------------------------------------------ lifecycle

    @Override
    protected void onCreate(Bundle saved) {
        duplicate = sLive != null && !sLive.isFinishing() && !sLive.isDestroyed();
        super.onCreate(saved); // loads the native library
        if (duplicate) {
            // Android restored a second instance (e.g. from an old task in recents). The viewer
            // can't run twice in one process: pass the file on and close this one.
            MainActivity live = sLive;
            live.handleIntent(getIntent(), true);
            finish();
            return;
        }
        sLive = this;
        Window w = getWindow();
        if (Build.VERSION.SDK_INT >= 28) {
            WindowManager.LayoutParams lp = w.getAttributes();
            lp.layoutInDisplayCutoutMode = Build.VERSION.SDK_INT >= 30
                    ? WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
                    : WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            w.setAttributes(lp);
        }
        w.getDecorView().setOnApplyWindowInsetsListener((v, insets) -> {
            sendInsets(insets);
            return v.onApplyWindowInsets(insets);
        });
        hideSystemBars();
        // Back goes to the viewer (closes the help, else sends the app to the background). It
        // must never finish the activity: see onDestroy.
        if (Build.VERSION.SDK_INT >= 33) {
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT, MainActivity::nativeBack);
        }

        Intent i = getIntent();
        boolean fromHistory = i != null && (i.getFlags() & Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0;
        if (saved != null || fromHistory || !handleIntent(i, true)) {
            restoreLast();
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        if (duplicate) {
            return;
        }
        setIntent(intent);
        handleIntent(intent, false);
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus && !duplicate) {
            hideSystemBars();
        }
    }

    /** Android 12 and older, when the Back key wasn't already handled by the viewer. */
    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        nativeBack();
    }

    /**
     * The Rust viewer runs one event loop per process, and it can't be started a second time.
     * Every config change is handled in place (see the manifest), so the activity only goes away
     * when it is finished or removed from recents; end the process then, and the next launch
     * starts clean (and reopens the last file).
     */
    @Override
    protected void onDestroy() {
        io.shutdownNow();
        if (duplicate) {
            super.onDestroy();
            return;
        }
        Process.killProcess(Process.myPid());
        super.onDestroy();
    }

    // ------------------------------------------------------------------ window

    @SuppressWarnings("deprecation")
    private void hideSystemBars() {
        Window w = getWindow();
        if (Build.VERSION.SDK_INT >= 30) {
            w.setDecorFitsSystemWindows(false);
            WindowInsetsController c = w.getInsetsController();
            if (c != null) {
                c.hide(WindowInsets.Type.systemBars());
                c.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        } else {
            w.getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                    | View.SYSTEM_UI_FLAG_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
        }
    }

    /** Safe area for the UI: the camera cutout, plus the system bars while they are swiped in. */
    private void sendInsets(WindowInsets insets) {
        int l = 0, t = 0, r = 0, b = 0;
        if (Build.VERSION.SDK_INT >= 30) {
            Insets s = insets.getInsets(WindowInsets.Type.displayCutout() | WindowInsets.Type.systemBars());
            l = s.left; t = s.top; r = s.right; b = s.bottom;
        } else if (Build.VERSION.SDK_INT >= 28) {
            DisplayCutout c = insets.getDisplayCutout();
            if (c != null) {
                l = c.getSafeInsetLeft(); t = c.getSafeInsetTop(); r = c.getSafeInsetRight(); b = c.getSafeInsetBottom();
            }
        }
        nativeInsets(l, t, r, b);
    }

    // ------------------------------------------------------------------ called from Rust

    public void pickSchematic() {
        runOnUiThread(() -> startPicker(REQ_OPEN));
    }

    public void pickJar() {
        runOnUiThread(() -> startPicker(REQ_JAR));
    }

    public void goBack() {
        runOnUiThread(() -> moveTaskToBack(true));
    }

    public void downloadTextures() {
        if (downloading) {
            return;
        }
        downloading = true;
        io.execute(() -> {
            try {
                download();
            } catch (Exception e) {
                Log.e(TAG, "texture download failed", e);
                status("Download failed: " + describe(e), false);
            } finally {
                downloading = false;
            }
        });
    }

    // ------------------------------------------------------------------ files

    private void startPicker(int req) {
        Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        i.addCategory(Intent.CATEGORY_OPENABLE);
        i.setType("*/*");
        try {
            startActivityForResult(i, req);
        } catch (ActivityNotFoundException e) {
            Intent g = new Intent(Intent.ACTION_GET_CONTENT);
            g.addCategory(Intent.CATEGORY_OPENABLE);
            g.setType("*/*");
            try {
                startActivityForResult(Intent.createChooser(g, null), req);
            } catch (ActivityNotFoundException e2) {
                status("No file picker found on this phone.", false);
            }
        }
    }

    @Override
    protected void onActivityResult(int req, int result, Intent data) {
        super.onActivityResult(req, result, data);
        if (result != RESULT_OK || data == null || data.getData() == null) {
            return;
        }
        Uri uri = data.getData();
        if (req == REQ_OPEN) {
            importSchematic(uri, false);
        } else if (req == REQ_JAR) {
            importJar(uri);
        }
    }

    /** VIEW ("Open with") and SEND ("Share") intents. Returns true if it carried a file. */
    @SuppressWarnings("deprecation")
    private boolean handleIntent(Intent i, boolean atStart) {
        if (i == null) {
            return false;
        }
        Uri uri = null;
        if (Intent.ACTION_VIEW.equals(i.getAction())) {
            uri = i.getData();
        } else if (Intent.ACTION_SEND.equals(i.getAction())) {
            uri = Build.VERSION.SDK_INT >= 33
                    ? i.getParcelableExtra(Intent.EXTRA_STREAM, Uri.class)
                    : (Uri) i.getParcelableExtra(Intent.EXTRA_STREAM);
        }
        if (uri == null) {
            return false;
        }
        importSchematic(uri, atStart);
        return true;
    }

    /**
     * Copy the file into files/last/ (replacing the previous one) and open it from there.
     * Keeping it means the viewer can show it again after Android has closed the app.
     */
    private void importSchematic(Uri uri, boolean atStart) {
        io.execute(() -> {
            String name = displayName(uri, "schematic");
            try {
                status("Copying " + name + "…", true);
                File dir = new File(getFilesDir(), "last");
                File part = new File(dir, ".incoming");
                copy(uri, part, dir);
                for (File f : list(dir)) {
                    if (!f.getName().equals(part.getName())) {
                        f.delete();
                    }
                }
                File dst = new File(dir, name);
                if (!part.renameTo(dst)) {
                    throw new IOException("cannot save the file");
                }
                status(null, false);
                nativeOpen(dst.getAbsolutePath());
            } catch (SecurityException e) {
                if (atStart) {
                    // Android re-sent an old "Open with" whose read permission has expired (the
                    // process was restarted). Show the last file instead of an error.
                    Log.w(TAG, "stale intent: " + e.getMessage());
                    status(null, false);
                    restoreLast();
                } else {
                    Log.e(TAG, "import failed", e);
                    status("Could not read " + name + ": the other app did not allow access", false);
                }
            } catch (Exception e) {
                Log.e(TAG, "import failed", e);
                status("Could not read " + name + ": " + describe(e), false);
            }
        });
    }

    private void restoreLast() {
        io.execute(() -> {
            for (File f : list(new File(getFilesDir(), "last"))) {
                if (!f.getName().startsWith(".")) {
                    nativeOpen(f.getAbsolutePath());
                    return;
                }
            }
        });
    }

    private void importJar(Uri uri) {
        if (downloading) {
            return;
        }
        downloading = true;
        io.execute(() -> {
            String name = displayName(uri, "client.jar");
            try {
                status("Reading " + name + "…", true);
                File dir = jarDir();
                File part = new File(dir, "import.part");
                copy(uri, part, dir);
                install(part, null);
            } catch (Exception e) {
                Log.e(TAG, "jar import failed", e);
                status("Could not use " + name + ": " + describe(e), false);
            } finally {
                downloading = false;
            }
        });
    }

    // ------------------------------------------------------------------ textures

    private File jarDir() {
        // Must match SCHEMVIEW_JAR_DIR in src/app.rs (android_main).
        return new File(getFilesDir(), "minecraft");
    }

    /** Newest release client jar from Mojang's launcher metadata, checked against its SHA-1. */
    private void download() throws Exception {
        status("Looking up the newest Minecraft version…", true);
        JSONObject manifest = new JSONObject(new String(fetch(MANIFEST, null), "UTF-8"));
        String id = manifest.getJSONObject("latest").getString("release");
        JSONArray versions = manifest.getJSONArray("versions");
        JSONObject entry = null;
        for (int k = 0; k < versions.length(); k++) {
            if (id.equals(versions.getJSONObject(k).getString("id"))) {
                entry = versions.getJSONObject(k);
                break;
            }
        }
        if (entry == null) {
            throw new IOException("version " + id + " missing from Mojang's list");
        }
        JSONObject version = new JSONObject(new String(fetch(entry.getString("url"), entry.optString("sha1", null)), "UTF-8"));
        JSONObject client = version.getJSONObject("downloads").getJSONObject("client");
        String url = client.getString("url");
        String sha1 = client.getString("sha1");
        long size = client.optLong("size", -1);

        File dir = jarDir();
        File have = new File(dir, safeName(id) + ".jar");
        if (have.isFile() && have.length() == size && sha1.equals(hex(digest(have)))) {
            status("Textures are up to date (Minecraft " + id + ").", false);
            nativeTexturesChanged();
            return;
        }
        File part = new File(dir, "download.part");
        mkdirs(dir);
        HttpURLConnection c = open(url);
        try (InputStream in = c.getInputStream(); OutputStream out = new FileOutputStream(part)) {
            long total = size > 0 ? size : c.getContentLengthLong();
            MessageDigest md = MessageDigest.getInstance("SHA-1");
            byte[] buf = new byte[1 << 16];
            long done = 0;
            int shown = -1;
            for (int n; (n = in.read(buf)) > 0; ) {
                out.write(buf, 0, n);
                md.update(buf, 0, n);
                done += n;
                int pct = total > 0 ? (int) (done * 100 / total) : -1;
                if (pct != shown) {
                    shown = pct;
                    String mb = String.format("%.0f", total / 1e6);
                    status(pct >= 0 ? "Downloading Minecraft " + id + " textures… " + pct + " % of " + mb + " MB"
                            : "Downloading Minecraft " + id + " textures… " + done / 1000000 + " MB", true);
                }
            }
            if (!sha1.equals(hex(md.digest()))) {
                part.delete();
                throw new IOException("the download was damaged (checksum mismatch), try again");
            }
        } finally {
            c.disconnect();
        }
        install(part, id);
    }

    /** Check that the jar has block models and textures, then make it the only jar in jarDir(). */
    private void install(File part, String id) throws Exception {
        try (ZipFile z = new ZipFile(part)) {
            if (z.getEntry("assets/minecraft/blockstates/stone.json") == null
                    || z.getEntry("assets/minecraft/textures/block/stone.png") == null) {
                throw new IOException("this is not a Minecraft 1.13+ client jar (no block models inside)");
            }
            if (id == null) {
                ZipEntry v = z.getEntry("version.json");
                if (v != null) {
                    try (InputStream in = z.getInputStream(v)) {
                        JSONObject j = new JSONObject(new String(readAll(in), "UTF-8"));
                        id = j.optString("id", j.optString("name", null));
                    }
                }
            }
        } catch (IOException e) {
            part.delete();
            throw e;
        }
        if (id == null || id.isEmpty()) {
            id = "custom";
        }
        File dir = jarDir();
        File dst = new File(dir, safeName(id) + ".jar");
        dst.delete();
        if (!part.renameTo(dst)) {
            throw new IOException("cannot save the jar");
        }
        for (File f : list(dir)) {
            if (!f.equals(dst)) {
                f.delete();
            }
        }
        status("Block textures from Minecraft " + id + " installed.", false);
        nativeTexturesChanged();
    }

    // ------------------------------------------------------------------ helpers

    private static HttpURLConnection open(String url) throws IOException {
        URL u = new URL(url);
        // Only Mojang's own HTTPS servers.
        if (!"https".equals(u.getProtocol()) || !(u.getHost().equals("mojang.com") || u.getHost().endsWith(".mojang.com"))) {
            throw new IOException("unexpected download address " + u.getHost());
        }
        HttpURLConnection c = (HttpURLConnection) u.openConnection();
        c.setConnectTimeout(15000);
        c.setReadTimeout(30000);
        c.setRequestProperty("User-Agent", "schemview-android");
        int code = c.getResponseCode();
        if (code != 200) {
            c.disconnect();
            throw new IOException("server answered " + code);
        }
        return c;
    }

    private static byte[] fetch(String url, String sha1) throws Exception {
        HttpURLConnection c = open(url);
        try (InputStream in = c.getInputStream()) {
            byte[] data = readAll(in);
            if (sha1 != null && !sha1.isEmpty()) {
                MessageDigest md = MessageDigest.getInstance("SHA-1");
                if (!sha1.equals(hex(md.digest(data)))) {
                    throw new IOException("checksum mismatch for " + url);
                }
            }
            return data;
        } finally {
            c.disconnect();
        }
    }

    private void copy(Uri uri, File dst, File dir) throws IOException {
        mkdirs(dir);
        try (InputStream in = getContentResolver().openInputStream(uri);
             OutputStream out = new FileOutputStream(dst)) {
            if (in == null) {
                throw new IOException("the other app gave no data");
            }
            byte[] buf = new byte[1 << 16];
            for (int n; (n = in.read(buf)) > 0; ) {
                out.write(buf, 0, n);
            }
        } catch (IOException | RuntimeException e) {
            dst.delete();
            throw e;
        }
    }

    private String displayName(Uri uri, String fallback) {
        String name = null;
        if ("content".equals(uri.getScheme())) {
            try (Cursor c = getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
                if (c != null && c.moveToFirst() && !c.isNull(0)) {
                    name = c.getString(0);
                }
            } catch (RuntimeException e) {
                Log.w(TAG, "no display name", e);
            }
        }
        if (name == null) {
            name = uri.getLastPathSegment();
        }
        name = name == null ? "" : safeName(name.substring(name.lastIndexOf('/') + 1));
        return name.isEmpty() || name.startsWith(".") ? fallback : name;
    }

    /** A file name without path separators or control characters. */
    private static String safeName(String s) {
        StringBuilder b = new StringBuilder();
        for (char ch : s.toCharArray()) {
            b.append(ch < 32 || ch == '/' || ch == '\\' || ch == ':' ? '_' : ch);
        }
        String r = b.toString().trim();
        return r.length() > 120 ? r.substring(r.length() - 120) : r;
    }

    private static File[] list(File dir) {
        File[] f = dir.listFiles();
        return f == null ? new File[0] : f;
    }

    private static void mkdirs(File dir) throws IOException {
        if (!dir.isDirectory() && !dir.mkdirs()) {
            throw new IOException("cannot create " + dir);
        }
    }

    private static byte[] readAll(InputStream in) throws IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        byte[] buf = new byte[1 << 16];
        for (int n; (n = in.read(buf)) > 0; ) {
            out.write(buf, 0, n);
        }
        return out.toByteArray();
    }

    private static byte[] digest(File f) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-1");
        try (InputStream in = new java.io.FileInputStream(f)) {
            byte[] buf = new byte[1 << 16];
            for (int n; (n = in.read(buf)) > 0; ) {
                md.update(buf, 0, n);
            }
        }
        return md.digest();
    }

    private static String hex(byte[] d) {
        StringBuilder b = new StringBuilder();
        for (byte x : d) {
            b.append(String.format("%02x", x & 0xff));
        }
        return b.toString();
    }

    private static String describe(Exception e) {
        if (e instanceof java.net.UnknownHostException) {
            return "no internet connection";
        }
        if (e instanceof java.net.SocketTimeoutException) {
            return "the server did not answer in time";
        }
        String m = e.getMessage();
        return m == null || m.isEmpty() ? e.getClass().getSimpleName() : m;
    }

    private static void status(String text, boolean sticky) {
        nativeStatus(text, sticky);
    }
}
