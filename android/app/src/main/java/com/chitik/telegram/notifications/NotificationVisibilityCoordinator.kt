package com.chitik.telegram.notifications

/** Serializes the planner and publisher visibility updates against live message delivery. */
internal class NotificationVisibilityCoordinator {
    private val lock = Any()

    /** Applies both visibility views as one transition relative to notification delivery. */
    fun updateVisibility(updatePlanner: () -> Unit, updatePublisher: () -> Unit) {
        synchronized(lock) {
            updatePlanner()
            updatePublisher()
        }
    }

    /** Runs the event decision and publish step against one consistent visibility transition. */
    fun withStableVisibility(deliver: () -> Unit) = synchronized(lock, deliver)
}
