package com.chitik.telegram

import android.content.Context
import android.provider.ContactsContract
import android.telephony.PhoneNumberUtils
import android.telephony.TelephonyManager
import com.chitik.telegram.nativebridge.PhoneContactInput
import java.util.Locale

internal const val MAX_PHONE_CONTACT_IMPORT = 5_000

internal fun readPhoneContacts(context: Context): List<PhoneContactInput> {
    val region = phoneNumberRegion(context)
    val unique = LinkedHashMap<String, Pair<String, String>>()
    val projection = arrayOf(
        ContactsContract.CommonDataKinds.Phone.DISPLAY_NAME,
        ContactsContract.CommonDataKinds.Phone.NUMBER,
    )

    context.contentResolver.query(
        ContactsContract.CommonDataKinds.Phone.CONTENT_URI,
        projection,
        null,
        null,
        ContactsContract.CommonDataKinds.Phone.DISPLAY_NAME,
    )?.use { cursor ->
        val nameColumn = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Phone.DISPLAY_NAME)
        val numberColumn = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Phone.NUMBER)
        if (nameColumn >= 0 && numberColumn >= 0) {
            while (cursor.moveToNext() && unique.size < MAX_PHONE_CONTACT_IMPORT) {
                val rawNumber = cursor.getString(numberColumn).orEmpty()
                val formatted = runCatching {
                    PhoneNumberUtils.formatNumberToE164(rawNumber, region)
                }.getOrNull()
                val phone = normalizeE164Candidate(rawNumber, formatted) ?: continue
                val name = splitPhoneContactName(cursor.getString(nameColumn).orEmpty())
                unique.putIfAbsent(phone, name)
            }
        }
    }

    return unique.entries.mapIndexed { index, entry ->
        PhoneContactInput(
            clientId = index + 1L,
            phone = entry.key,
            firstName = entry.value.first,
            lastName = entry.value.second,
        )
    }
}

internal fun normalizeE164Candidate(rawNumber: String, formattedE164: String?): String? {
    val raw = rawNumber.trim()
    val fallback = raw.takeIf {
        it.startsWith("+") && it.drop(1).all { char ->
            char in '0'..'9' || char.isWhitespace() || char in "-()."
        }
    }?.let { value ->
        "+" + value.drop(1).filter { it in '0'..'9' }
    }
    val candidate = formattedE164 ?: fallback ?: return null
    if (!candidate.startsWith("+")) return null
    val digits = candidate.drop(1)
    return candidate.takeIf {
        digits.length in 7..15 &&
            digits.firstOrNull()?.let { first -> first in '1'..'9' } == true &&
            digits.all { digit -> digit in '0'..'9' }
    }
}

internal fun splitPhoneContactName(displayName: String): Pair<String, String> {
    val parts = displayName.trim().ifBlank { "Contact" }.split(Regex("\\s+"), limit = 2)
    return parts.first().take(64) to parts.getOrNull(1).orEmpty().take(64)
}

private fun phoneNumberRegion(context: Context): String {
    val telephony = runCatching {
        context.getSystemService(TelephonyManager::class.java)
    }.getOrNull()
    val network = runCatching { telephony?.networkCountryIso }.getOrNull().orEmpty()
    val sim = runCatching { telephony?.simCountryIso }.getOrNull().orEmpty()
    return sequenceOf(network, sim, Locale.getDefault().country)
        .map { it.trim() }
        .firstOrNull { it.length == 2 }
        ?.uppercase(Locale.ROOT)
        .orEmpty()
}
