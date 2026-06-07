package com.blumi.blugo

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.RadialGradient
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.service.wallpaper.WallpaperService
import android.view.SurfaceHolder
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

/**
 * blumi "Bloom" live wallpaper.
 *
 * The eight-petal blumi flower blooms — petal by petal — into the full logo
 * (flower + wordmark) on the Living-Rose dark background. The bloom replays
 * whenever the wallpaper becomes visible and, on foldables, the moment the
 * device is **opened** (detected via the hinge-angle sensor): fold open → bloom.
 *
 * Pure Canvas/2D, no Flutter — it just rides along in the blugo APK so it shows
 * up in the system live-wallpaper picker.
 */
class BloomWallpaperService : WallpaperService() {
    override fun onCreateEngine(): Engine = BloomEngine()

    inner class BloomEngine : WallpaperService.Engine(), SensorEventListener {
        private val handler = Handler(Looper.getMainLooper())
        private var visible = false
        private var bloomStart = 0L
        private val bloomDurMs = 2000L

        private var sensorManager: SensorManager? = null
        private var hinge: Sensor? = null
        private var lastHinge = -1f

        // Living-Rose ramp.
        private val rose = Color.parseColor("#FF4F87")
        private val lav = Color.parseColor("#9B86FF")
        private val violet = Color.parseColor("#6B50FF")
        private val cyan = Color.parseColor("#68FFD6")
        private val eyeDark = Color.parseColor("#0E1116")

        // Petals in flower-local space (±70): cx, cy, rx, ry, degrees.
        private val petals = arrayOf(
            floatArrayOf(0f, -36f, 19f, 33f, 0f),
            floatArrayOf(0f, 36f, 19f, 33f, 0f),
            floatArrayOf(-36f, 0f, 33f, 19f, 0f),
            floatArrayOf(36f, 0f, 33f, 19f, 0f),
            floatArrayOf(-26f, -26f, 28f, 15f, 45f),
            floatArrayOf(26f, 26f, 28f, 15f, 45f),
            floatArrayOf(26f, -26f, 28f, 15f, -45f),
            floatArrayOf(-26f, 26f, 28f, 15f, -45f),
        )

        private val bgPaint = Paint()
        private val glowPaint = Paint(Paint.ANTI_ALIAS_FLAG)
        private val petalPaint = Paint(Paint.ANTI_ALIAS_FLAG)
        private val nucPaint = Paint(Paint.ANTI_ALIAS_FLAG)
        private val eyePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = eyeDark }
        private val wordPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            textAlign = Paint.Align.CENTER
            typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
        }

        private val drawRunnable = Runnable { drawFrame() }

        override fun onCreate(holder: SurfaceHolder) {
            super.onCreate(holder)
            sensorManager =
                this@BloomWallpaperService.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
            if (Build.VERSION.SDK_INT >= 30) {
                hinge = sensorManager?.getDefaultSensor(Sensor.TYPE_HINGE_ANGLE)
            }
        }

        override fun onVisibilityChanged(v: Boolean) {
            visible = v
            if (v) {
                hinge?.let {
                    sensorManager?.registerListener(this, it, SensorManager.SENSOR_DELAY_NORMAL)
                }
                triggerBloom()
            } else {
                sensorManager?.unregisterListener(this)
                handler.removeCallbacks(drawRunnable)
            }
        }

        override fun onSurfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
            super.onSurfaceChanged(holder, format, width, height)
            triggerBloom()
        }

        override fun onDestroy() {
            handler.removeCallbacks(drawRunnable)
            sensorManager?.unregisterListener(this)
            super.onDestroy()
        }

        private fun triggerBloom() {
            bloomStart = SystemClock.uptimeMillis()
            if (visible) {
                handler.removeCallbacks(drawRunnable)
                handler.post(drawRunnable)
            }
        }

        override fun onSensorChanged(e: SensorEvent) {
            if (e.sensor?.type == Sensor.TYPE_HINGE_ANGLE) {
                val a = e.values[0]
                // folded ≈ 0°, flat ≈ 180°. Re-bloom on the opening transition.
                if (lastHinge in 0f..100f && a > 110f) triggerBloom()
                lastHinge = a
            }
        }

        override fun onAccuracyChanged(s: Sensor?, accuracy: Int) {}

        private fun easeOutBack(t: Float): Float {
            val c1 = 1.70158f
            val c3 = c1 + 1f
            val x = t - 1f
            return 1f + c3 * x * x * x + c1 * x * x
        }

        private fun easeOutCubic(t: Float): Float = 1f - (1f - t).pow(3)

        private fun drawFrame() {
            val holder = surfaceHolder
            var canvas: Canvas? = null
            try {
                canvas = holder.lockCanvas()
                if (canvas != null) render(canvas)
            } catch (_: Throwable) {
                // surface may be gone mid-draw; ignore and stop scheduling
            } finally {
                if (canvas != null) {
                    try {
                        holder.unlockCanvasAndPost(canvas)
                    } catch (_: Throwable) {
                    }
                }
            }
            handler.removeCallbacks(drawRunnable)
            val elapsed = SystemClock.uptimeMillis() - bloomStart
            // Animate through the bloom, then settle to a static logo (battery-friendly).
            if (visible && elapsed < bloomDurMs + 250L) {
                handler.postDelayed(drawRunnable, 16L)
            }
        }

        private fun render(c: Canvas) {
            val w = c.width.toFloat()
            val h = c.height.toFloat()
            val now = SystemClock.uptimeMillis()
            val t = ((now - bloomStart).toFloat() / bloomDurMs).coerceIn(0f, 1f)

            // 1) Living-Rose dark background.
            bgPaint.shader = RadialGradient(
                w * 0.5f, h * 0.42f, max(w, h) * 0.85f,
                intArrayOf(
                    Color.parseColor("#241129"),
                    Color.parseColor("#140A16"),
                    Color.parseColor("#08060C"),
                ),
                floatArrayOf(0f, 0.55f, 1f), Shader.TileMode.CLAMP,
            )
            c.drawRect(0f, 0f, w, h, bgPaint)

            val cx = w * 0.5f
            val cy = h * 0.44f
            val extent = min(w, h) * 0.20f       // bloom radius
            val scaleBase = extent / 70f          // petals live in ±70 space

            // 2) Bloom glow that swells with the flower.
            val glowT = easeOutCubic(t)
            val glowR = extent * (0.4f + 1.7f * glowT)
            glowPaint.shader = RadialGradient(
                cx, cy, glowR,
                intArrayOf(
                    Color.argb((150 * glowT).toInt(), 255, 120, 170),
                    Color.argb((70 * glowT).toInt(), 140, 110, 255),
                    Color.argb(0, 104, 255, 214),
                ),
                floatArrayOf(0f, 0.5f, 1f), Shader.TileMode.CLAMP,
            )
            c.drawCircle(cx, cy, glowR, glowPaint)

            // 3) The flower blooms open: whole group scales 0→1 with a slight
            //    spin, each petal staggered for a "petals unfurling" feel.
            c.save()
            c.translate(cx, cy)
            c.rotate((1f - easeOutCubic(t)) * -90f)
            val groupScale = easeOutBack(t) * scaleBase
            c.scale(groupScale, groupScale)

            val grad = LinearGradient(
                -70f, -70f, 70f, 70f,
                intArrayOf(rose, lav, violet, cyan),
                floatArrayOf(0f, 0.45f, 0.75f, 1f), Shader.TileMode.CLAMP,
            )
            petalPaint.shader = grad
            for (i in petals.indices) {
                val p = petals[i]
                val pt = ((t * 1.5f) - i * 0.07f).coerceIn(0f, 1f)
                val pe = easeOutBack(pt)
                petalPaint.alpha = (255 * easeOutCubic(pt)).toInt().coerceIn(0, 255)
                c.save()
                c.translate(p[0], p[1])
                c.rotate(p[4])
                c.scale(pe, pe)
                c.drawOval(RectF(-p[2], -p[3], p[2], p[3]), petalPaint)
                c.restore()
            }

            // 4) Nucleus pops in last.
            val nucT = ((t * 1.5f) - 0.55f).coerceIn(0f, 1f)
            if (nucT > 0f) {
                nucPaint.color = cyan
                nucPaint.alpha = (255 * nucT).toInt()
                c.drawCircle(0f, 0f, 17f * easeOutBack(nucT), nucPaint)
                eyePaint.alpha = (255 * nucT).toInt()
                c.drawCircle(0f, 0f, 7.5f * easeOutBack(nucT), eyePaint)
            }
            c.restore()

            // 5) Wordmark fades up beneath the bloom.
            val wt = ((t - 0.6f) / 0.4f).coerceIn(0f, 1f)
            if (wt > 0f) {
                val ts = extent * 0.66f
                wordPaint.textSize = ts
                val baseY = cy + extent + ts * 1.5f + (1f - wt) * 14f
                wordPaint.shader = LinearGradient(
                    cx - ts * 2.3f, baseY, cx + ts * 2.3f, baseY,
                    intArrayOf(rose, lav, violet, cyan),
                    floatArrayOf(0f, 0.38f, 0.66f, 1f), Shader.TileMode.CLAMP,
                )
                wordPaint.alpha = (255 * wt).toInt()
                c.drawText("blumi", cx, baseY, wordPaint)
            }
        }
    }
}
