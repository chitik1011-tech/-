package com.chitik.telegram.notifications

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationVisibilityCoordinatorTest {
    @Test
    fun deliveryCannotObserveHalfAppliedVisibilityTransition() {
        val coordinator = NotificationVisibilityCoordinator()
        var plannerVisibleChat: Long? = 41L
        var publisherVisibleChat: Long? = 41L
        val plannerUpdated = CountDownLatch(1)
        val finishPublisherUpdate = CountDownLatch(1)
        val deliveryFinished = CountDownLatch(1)
        var deliverySawConsistentState = false

        val updateThread = thread {
            coordinator.updateVisibility(
                updatePlanner = {
                    plannerVisibleChat = null
                    plannerUpdated.countDown()
                    assertTrue(finishPublisherUpdate.await(2, TimeUnit.SECONDS))
                },
                updatePublisher = { publisherVisibleChat = null },
            )
        }
        assertTrue(plannerUpdated.await(2, TimeUnit.SECONDS))

        val deliveryThread = thread {
            coordinator.withStableVisibility {
                deliverySawConsistentState = plannerVisibleChat == publisherVisibleChat
            }
            deliveryFinished.countDown()
        }

        assertFalse(deliveryFinished.await(100, TimeUnit.MILLISECONDS))
        finishPublisherUpdate.countDown()
        updateThread.join(2_000)
        deliveryThread.join(2_000)

        assertTrue(deliveryFinished.await(2, TimeUnit.SECONDS))
        assertTrue(deliverySawConsistentState)
        assertEquals(null, plannerVisibleChat)
        assertEquals(null, publisherVisibleChat)
    }
}
