package com.plugin.mediaparser

import java.io.IOException

class FaultInjectingStream(maximum: Int) : BoundedJpegOutputStream(maximum) {
    companion object {
        private val mode = ThreadLocal<Int>()
        @JvmStatic fun fault(value: Int) { mode.set(value) }
    }
    private var copies = 0
    override fun copyBuffer(capacity: Int): ByteArray {
        copies++
        val selected = mode.get() ?: 0
        if (selected == 1 || (selected == 2 && capacity == size)) throw OutOfMemoryError("injected allocation failure")
        return super.copyBuffer(capacity)
    }
}

object BoundedStreamCases {
    @JvmStatic fun run() {
        val bounded = BoundedJpegOutputStream(3)
        bounded.write(byteArrayOf(1, 2, 3), 0, 3)
        check(bounded.toByteArray().contentEquals(byteArrayOf(1, 2, 3)))
        try { bounded.write(4); error("limit accepted") } catch (_: IOException) {}
        check(bounded.failure == 1 && bounded.size == 3)
        try { bounded.write(5); error("poisoned stream accepted") } catch (_: IOException) {}
        check(bounded.failure == 1 && bounded.size == 3)
        FaultInjectingStream.fault(1)
        val allocation = FaultInjectingStream(10)
        try { allocation.write(1); error("allocation accepted") } catch (_: IOException) {}
        check(allocation.failure == 2 && allocation.size == 0)
        try { allocation.write(ByteArray(11), 0, 11); error("poisoned stream accepted") } catch (_: IOException) {}
        check(allocation.failure == 2)
        FaultInjectingStream.fault(0)
        val finalCopy = FaultInjectingStream(10)
        finalCopy.write(byteArrayOf(1, 2), 0, 2)
        FaultInjectingStream.fault(2)
        try { finalCopy.toByteArray(); error("copy accepted") } catch (_: IOException) {}
        check(finalCopy.failure == 2 && finalCopy.size == 2)
        FaultInjectingStream.fault(0)
        println("PASS Kotlin stream contracts")
    }
}
