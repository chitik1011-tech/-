package com.chitik.telegram

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.clickable
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

data class ContactListEntry(
    val userId: Long,
    val displayName: String,
    val username: String?,
    val isMutual: Boolean,
    val isBot: Boolean = false,
    val avatarPhotoId: Long? = null,
)

@Composable
fun ContactsScreen(
    contacts: List<ContactListEntry>,
    total: Int,
    isLoading: Boolean,
    error: String?,
    onBack: () -> Unit,
    onRefresh: () -> Unit,
    onLoadMore: () -> Unit,
    onContactSelected: (ContactListEntry) -> Unit,
    modifier: Modifier = Modifier,
) {
    var searchQuery by rememberSaveable { mutableStateOf("") }
    val visibleContacts = remember(contacts, searchQuery) {
        filterLoadedContacts(contacts, searchQuery)
    }
    Column(
        modifier = modifier.fillMaxSize(),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            TextButton(
                onClick = onBack,
                modifier = Modifier.semantics { contentDescription = "Return to chats" },
            ) { Text("Chats") }
            Text(
                "Contacts",
                modifier = Modifier.semantics { heading() },
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
            )
            OutlinedButton(
                onClick = onRefresh,
                enabled = !isLoading,
                modifier = Modifier.semantics {
                    contentDescription = if (isLoading) {
                        contactsLoadingAccessibilityLabel(contacts.size, total)
                    } else {
                        "Refresh contacts"
                    }
                },
            ) {
                if (isLoading) CircularProgressIndicator()
                else Text("Refresh")
            }
        }
        OutlinedTextField(
            value = searchQuery,
            onValueChange = { searchQuery = it },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            singleLine = true,
            label = { Text("Search loaded contacts") },
            placeholder = { Text("Name or username") },
        )
        Text(
            "Search applies to contacts loaded so far.",
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.padding(horizontal = 16.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = "${contacts.size} of $total contacts",
            modifier = Modifier.padding(horizontal = 16.dp).semantics {
                contentDescription = contactsCountAccessibilityLabel(contacts.size, total)
                liveRegion = LiveRegionMode.Polite
            },
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (error != null) {
            Text(
                text = error,
                modifier = Modifier.semantics {
                    contentDescription = contactsErrorAccessibilityLabel(error)
                    liveRegion = LiveRegionMode.Polite
                },
                color = MaterialTheme.colorScheme.error,
            )
        }
        if (contacts.isEmpty() && isLoading) {
            CircularProgressIndicator(
                modifier = Modifier.padding(24.dp).semantics {
                    contentDescription = contactsLoadingAccessibilityLabel(contacts.size, total)
                    liveRegion = LiveRegionMode.Polite
                },
            )
        } else if (contacts.isEmpty()) {
            Text("No Telegram contacts found.", modifier = Modifier.padding(24.dp))
        } else if (visibleContacts.isEmpty()) {
            Text("No loaded contacts match this search.", modifier = Modifier.padding(24.dp))
        }
        LazyColumn(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            items(visibleContacts, key = ContactListEntry::userId) { contact ->
                Row(
                    modifier = Modifier.fillMaxWidth().clickable { onContactSelected(contact) }
                        .semantics(mergeDescendants = true) {
                            contentDescription = contactItemAccessibilityLabel(contact)
                        }
                        .padding(horizontal = TelegramUiTokens.horizontalInset, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    AvatarPlaceholder(contact.displayName, size = TelegramUiTokens.contactAvatar)
                    Spacer(Modifier.width(TelegramUiTokens.rowGap))
                    Column(modifier = Modifier.weight(1f)) {
                        Text(contact.displayName, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                        val subtitle = if (contact.isBot) "bot" else contact.username?.let { "@$it" }
                        subtitle?.let {
                            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
                HorizontalDivider(
                    modifier = Modifier.padding(start = TelegramUiTokens.dividerInset),
                    color = MaterialTheme.colorScheme.outlineVariant,
                    thickness = 0.5.dp,
                )
            }
            if (contacts.size < total) {
                item(key = "load_more_contacts") {
                    TextButton(
                        onClick = onLoadMore,
                        enabled = !isLoading,
                        modifier = Modifier.semantics {
                            contentDescription = contactsLoadMoreAccessibilityLabel(
                                contacts.size,
                                total,
                                isLoading,
                            )
                        },
                    ) {
                        Text(if (isLoading) "Loading…" else "Load more")
                    }
                }
            }
        }
    }
}
