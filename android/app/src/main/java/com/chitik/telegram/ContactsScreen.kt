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
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
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
    Column(
        modifier = modifier.fillMaxSize().padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            TextButton(onClick = onBack) { Text("Chats") }
            Text("Contacts", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
            OutlinedButton(onClick = onRefresh, enabled = !isLoading) {
                if (isLoading) CircularProgressIndicator()
                else Text("Refresh")
            }
        }
        Text("${contacts.size} of $total", style = MaterialTheme.typography.bodySmall)
        if (error != null) Text(error, color = MaterialTheme.colorScheme.error)
        if (contacts.isEmpty() && isLoading) {
            CircularProgressIndicator(modifier = Modifier.padding(24.dp))
        } else if (contacts.isEmpty()) {
            Text("No Telegram contacts found.", modifier = Modifier.padding(24.dp))
        }
        LazyColumn(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            items(contacts, key = ContactListEntry::userId) { contact ->
                Card(
                    onClick = { onContactSelected(contact) },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        AvatarPlaceholder(contact.displayName, size = 44.dp)
                        Spacer(Modifier.width(12.dp))
                        Column {
                            Text(contact.displayName, style = MaterialTheme.typography.titleMedium)
                            contact.username?.let { Text("@$it", style = MaterialTheme.typography.bodySmall) }
                            if (contact.isBot) Text("Bot", style = MaterialTheme.typography.labelSmall)
                        }
                    }
                }
            }
            if (contacts.size < total) {
                item(key = "load_more_contacts") {
                    TextButton(onClick = onLoadMore, enabled = !isLoading) {
                        Text(if (isLoading) "Loading…" else "Load more")
                    }
                }
            }
        }
    }
}
