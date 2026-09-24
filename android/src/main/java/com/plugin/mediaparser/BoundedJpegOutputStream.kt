package com.plugin.mediaparser

import java.io.IOException
import java.io.OutputStream
import java.util.Arrays

/** Bounded JPEG destination. Failure is sticky even if Bitmap.compress swallows it. */
open class BoundedJpegOutputStream(private val maximum: Int) : OutputStream() {
    private var data = ByteArray(0)
    @JvmField var failure: Int = 0
    @JvmField var size: Int = 0

    init { require(maximum >= 0) }

    private fun reserve(count: Int) {
        if (failure != 0) throw IOException("JPEG stream already failed")
        if (count > maximum - size) {
            failure = 1
            throw IOException("JPEG output is too large")
        }
        val required = size + count
        if (required > data.size) {
            val capacity = minOf(maximum.toLong(), maxOf(required.toLong(), data.size.toLong() * 2)).toInt()
            try { data = copyBuffer(capacity) }
            catch (error: OutOfMemoryError) {
                failure = 2
                throw IOException("JPEG output allocation failed", error)
            }
        }
    }

    // An overridable allocation boundary lets the JVM harness exercise genuine OOM handling.
    protected open fun copyBuffer(capacity: Int): ByteArray = Arrays.copyOf(data, capacity)

    override fun write(value: Int) {
        reserve(1)
        data[size++] = value.toByte()
    }

    override fun write(bytes: ByteArray, offset: Int, count: Int) {
        if (offset < 0 || count < 0 || offset > bytes.size - count) throw IndexOutOfBoundsException()
        reserve(count)
        System.arraycopy(bytes, offset, data, size, count)
        size += count
    }

    fun toByteArray(): ByteArray {
        if (failure != 0) throw IOException("JPEG stream already failed")
        try { return copyBuffer(size) }
        catch (error: OutOfMemoryError) {
            failure = 2
            throw IOException("JPEG output allocation failed", error)
        }
    }
}
