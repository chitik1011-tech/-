package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.assertNull
import org.junit.Test

class LocationSupportTest {
    @Test
    fun buildsSystemGeoIntentWithoutMapProviderDependency() {
        val spec = buildGeoIntentSpec(
            latitude = 52.2296756,
            longitude = 21.0122287,
            label = "Central Library",
        )

        requireNotNull(spec)
        assertEquals("android.intent.action.VIEW", spec.action)
        assertEquals(
            "geo:52.2296756,21.0122287?q=52.2296756,21.0122287(Central%20Library)",
            spec.uri,
        )
    }

    @Test
    fun invalidCoordinatesDoNotProduceAnIntent() {
        assertNull(buildGeoIntentSpec(Double.NaN, 0.0, null))
        assertNull(buildGeoIntentSpec(Double.POSITIVE_INFINITY, 0.0, null))
        assertNull(buildGeoIntentSpec(90.000001, 0.0, null))
        assertNull(buildGeoIntentSpec(0.0, -180.000001, null))
    }
}

class OneShotLocationCallbackTest {
    @Test
    fun cancellationSuppressesLateLocationResult() {
        val delivered = mutableListOf<String>()
        val callback = OneShotLocationCallback<String>(delivered::add)

        assertTrue(callback.cancel())
        callback.deliver("location")

        assertTrue(delivered.isEmpty())
        assertFalse(callback.isActive())
        assertFalse(callback.cancel())
    }

    @Test
    fun callbackDeliversOnlyFirstResult() {
        val delivered = mutableListOf<String>()
        val callback = OneShotLocationCallback<String>(delivered::add)

        callback.deliver("first")
        callback.deliver("late")

        assertEquals(listOf("first"), delivered)
    }
}
