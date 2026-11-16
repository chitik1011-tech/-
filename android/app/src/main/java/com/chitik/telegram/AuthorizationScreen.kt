package com.chitik.telegram

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp

/**
 * Mirrors the authorization states emitted by the Rust core.
 *
 * The parent must update [AuthorizationUiState.authorization] only from Rust events;
 * this screen never advances the authorization flow locally.
 */
enum class AuthorizationUiStep {
    Starting,
    WaitingForPhoneNumber,
    WaitingForCode,
    WaitingForPassword,
    Ready,
    LoggingOut,
    Closed,
}

/** UI projection of the backend-owned authorization state. */
data class AuthorizationUiState(
    val authorization: AuthorizationUiStep = AuthorizationUiStep.Closed,
    /** Caller-owned configuration readiness, sourced from its runtime config store. */
    val isTelegramConfigured: Boolean = false,
    val isLoading: Boolean = false,
    val error: String? = null,
)

/**
 * Telegram sign-in form. All actions are callbacks so the caller can send canonical
 * CoreCommand values to Rust and feed resulting AuthorizationChanged events back here.
 * Temporary login code and password input are kept only in Compose memory, not saved state.
 */
@Composable
fun AuthorizationScreen(
    state: AuthorizationUiState,
    onConfigureTelegram: (apiId: String, apiHash: String) -> Unit,
    onStart: () -> Unit,
    onSubmitPhoneNumber: (String) -> Unit,
    onSubmitCode: (String) -> Unit,
    onSubmitPassword: (String) -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // Credentials remain in Compose memory only and are intentionally not saveable.
    var apiId by remember { mutableStateOf("") }
    var apiHash by remember { mutableStateOf("") }
    var phoneNumber by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var validationError by remember { mutableStateOf<String?>(null) }
    val apiIdFocusRequester = remember { FocusRequester() }
    val apiHashFocusRequester = remember { FocusRequester() }
    val phoneFocusRequester = remember { FocusRequester() }
    val codeFocusRequester = remember { FocusRequester() }
    val passwordFocusRequester = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    val authInputStep = state.authorization == AuthorizationUiStep.WaitingForPhoneNumber ||
        state.authorization == AuthorizationUiStep.WaitingForCode ||
        state.authorization == AuthorizationUiStep.WaitingForPassword

    LaunchedEffect(state.isTelegramConfigured, state.authorization) {
        val fieldsToClear = authorizationInputsToClear(state)
        if (AuthorizationInputField.ApiId in fieldsToClear) apiId = ""
        if (AuthorizationInputField.ApiHash in fieldsToClear) apiHash = ""
        if (AuthorizationInputField.PhoneNumber in fieldsToClear) phoneNumber = ""
        if (AuthorizationInputField.LoginCode in fieldsToClear) code = ""
        if (AuthorizationInputField.TwoFactorPassword in fieldsToClear) password = ""
    }

    BackHandler(enabled = true) {
        keyboard?.hide()
        onBack()
    }

    LaunchedEffect(state.isTelegramConfigured, state.authorization, state.isLoading) {
        if (state.isLoading) {
            keyboard?.hide()
        } else if (!state.isTelegramConfigured) {
            apiIdFocusRequester.requestFocus()
            keyboard?.show()
        } else if (authInputStep) {
            when (state.authorization) {
                AuthorizationUiStep.WaitingForPhoneNumber -> phoneFocusRequester.requestFocus()
                AuthorizationUiStep.WaitingForCode -> codeFocusRequester.requestFocus()
                AuthorizationUiStep.WaitingForPassword -> passwordFocusRequester.requestFocus()
                else -> Unit
            }
            keyboard?.show()
        } else {
            keyboard?.hide()
        }
    }

    val title = if (!state.isTelegramConfigured) {
        "Configure Telegram"
    } else when (state.authorization) {
        AuthorizationUiStep.Starting -> "Connecting to Telegram"
        AuthorizationUiStep.WaitingForPhoneNumber -> "Your phone number"
        AuthorizationUiStep.WaitingForCode -> "Enter the login code"
        AuthorizationUiStep.WaitingForPassword -> "Two-step verification"
        AuthorizationUiStep.Ready -> "You're signed in"
        AuthorizationUiStep.LoggingOut -> "Signing out"
        AuthorizationUiStep.Closed -> "Sign in to Telegram"
    }
    val subtitle = if (!state.isTelegramConfigured) {
        "Enter the Telegram API ID and API hash. The caller stores them using the app's protected runtime configuration store."
    } else when (state.authorization) {
        AuthorizationUiStep.Starting -> "Waiting for the Rust client to initialize."
        AuthorizationUiStep.WaitingForPhoneNumber -> "Enter your number with the country code."
        AuthorizationUiStep.WaitingForCode -> "Enter the code Telegram sent to your account."
        AuthorizationUiStep.WaitingForPassword -> "Enter your Telegram two-step verification password."
        AuthorizationUiStep.Ready -> "Your Telegram account is connected."
        AuthorizationUiStep.LoggingOut -> "The session is closing."
        AuthorizationUiStep.Closed -> "Continue with your Telegram account."
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 28.dp, vertical = 32.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(18.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.headlineMedium,
                textAlign = TextAlign.Center,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                text = subtitle,
                style = MaterialTheme.typography.bodyLarge,
                textAlign = TextAlign.Center,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            if (!state.isTelegramConfigured) {
                val parsedApiId = apiId.trim().toIntOrNull()
                val normalizedApiHash = apiHash.trim()
                val apiIdValid = parsedApiId != null && parsedApiId > 0
                val apiHashValid = normalizedApiHash.length == 32 &&
                    normalizedApiHash.all { character -> character in '0'..'9' || character.lowercaseChar() in 'a'..'f' }

                OutlinedTextField(
                    value = apiId,
                    onValueChange = {
                        apiId = it.filter { character -> character in '0'..'9' }.take(10)
                        validationError = null
                    },
                    modifier = Modifier
                        .fillMaxWidth()
                        .focusRequester(apiIdFocusRequester),
                    label = { Text("API ID") },
                    singleLine = true,
                    enabled = !state.isLoading,
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Number,
                        imeAction = ImeAction.Next,
                    ),
                    keyboardActions = KeyboardActions(
                        onNext = { apiHashFocusRequester.requestFocus() },
                    ),
                )
                OutlinedTextField(
                    value = apiHash,
                    onValueChange = {
                        apiHash = it
                        validationError = null
                    },
                    modifier = Modifier
                        .fillMaxWidth()
                        .focusRequester(apiHashFocusRequester),
                    label = { Text("API hash") },
                    singleLine = true,
                    enabled = !state.isLoading,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Ascii,
                        imeAction = ImeAction.Done,
                    ),
                    keyboardActions = KeyboardActions(
                        onDone = {
                            if (!state.isLoading) {
                                if (apiIdValid && apiHashValid) {
                                    keyboard?.hide()
                                    onConfigureTelegram(apiId.trim(), normalizedApiHash)
                                } else {
                                    validationError = "Enter a positive numeric API ID and a 32-character hexadecimal API hash."
                                }
                            }
                        },
                    ),
                )
                Button(
                    onClick = {
                        if (apiIdValid && apiHashValid) {
                            validationError = null
                            keyboard?.hide()
                            onConfigureTelegram(apiId.trim(), normalizedApiHash)
                        } else {
                            validationError = "Enter a positive numeric API ID and a 32-character hexadecimal API hash."
                        }
                    },
                    enabled = !state.isLoading,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    if (state.isLoading) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(18.dp),
                            color = MaterialTheme.colorScheme.onPrimary,
                            strokeWidth = 2.dp,
                        )
                        Spacer(Modifier.width(8.dp))
                    }
                    Text("Connect Telegram")
                }
            } else {
                when (state.authorization) {
                AuthorizationUiStep.Closed -> {
                    Button(
                        onClick = onStart,
                        enabled = !state.isLoading,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        if (state.isLoading) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(18.dp),
                                color = MaterialTheme.colorScheme.onPrimary,
                                strokeWidth = 2.dp,
                            )
                            Spacer(Modifier.width(8.dp))
                        }
                        Text("Continue")
                    }
                }

                AuthorizationUiStep.WaitingForPhoneNumber -> {
                    OutlinedTextField(
                        value = phoneNumber,
                        onValueChange = { phoneNumber = it },
                        modifier = Modifier
                            .fillMaxWidth()
                            .focusRequester(phoneFocusRequester),
                        label = { Text("Phone number") },
                        placeholder = { Text("+1 555 123 4567") },
                        singleLine = true,
                        enabled = !state.isLoading,
                        keyboardOptions = KeyboardOptions(
                            keyboardType = KeyboardType.Phone,
                            imeAction = ImeAction.Done,
                        ),
                        keyboardActions = KeyboardActions(
                            onDone = {
                                val value = phoneNumber.trim()
                                if (value.isNotEmpty() && !state.isLoading) onSubmitPhoneNumber(value)
                            },
                        ),
                    )
                    Button(
                        onClick = {
                            val value = phoneNumber.trim()
                            if (value.isNotEmpty()) onSubmitPhoneNumber(value)
                        },
                        enabled = phoneNumber.isNotBlank() && !state.isLoading,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        if (state.isLoading) {
                            CircularProgressIndicator(
                                modifier = Modifier.height(18.dp),
                                color = MaterialTheme.colorScheme.onPrimary,
                                strokeWidth = 2.dp,
                            )
                        } else {
                            Text("Send code")
                        }
                    }
                }

                AuthorizationUiStep.WaitingForCode -> {
                    OutlinedTextField(
                        value = code,
                        onValueChange = { code = it },
                        modifier = Modifier
                            .fillMaxWidth()
                            .focusRequester(codeFocusRequester),
                        label = { Text("Login code") },
                        singleLine = true,
                        enabled = !state.isLoading,
                        keyboardOptions = KeyboardOptions(
                            keyboardType = KeyboardType.Number,
                            imeAction = ImeAction.Done,
                        ),
                        keyboardActions = KeyboardActions(
                            onDone = {
                                val value = code.trim()
                                if (value.isNotEmpty() && !state.isLoading) {
                                    onSubmitCode(value)
                                    code = ""
                                }
                            },
                        ),
                    )
                    Button(
                        onClick = {
                            val value = code.trim()
                            if (value.isNotEmpty()) {
                                onSubmitCode(value)
                                code = ""
                            }
                        },
                        enabled = code.isNotBlank() && !state.isLoading,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        if (state.isLoading) {
                            CircularProgressIndicator(
                                modifier = Modifier.height(18.dp),
                                color = MaterialTheme.colorScheme.onPrimary,
                                strokeWidth = 2.dp,
                            )
                        } else {
                            Text("Continue")
                        }
                    }
                }

                AuthorizationUiStep.WaitingForPassword -> {
                    OutlinedTextField(
                        value = password,
                        onValueChange = { password = it },
                        modifier = Modifier
                            .fillMaxWidth()
                            .focusRequester(passwordFocusRequester),
                        label = { Text("Password") },
                        singleLine = true,
                        enabled = !state.isLoading,
                        visualTransformation = PasswordVisualTransformation(),
                        keyboardOptions = KeyboardOptions(
                            keyboardType = KeyboardType.Password,
                            imeAction = ImeAction.Done,
                        ),
                        keyboardActions = KeyboardActions(
                            onDone = {
                                if (password.isNotEmpty() && !state.isLoading) {
                                    onSubmitPassword(password)
                                    password = ""
                                }
                            },
                        ),
                    )
                    Button(
                        onClick = {
                            if (password.isNotEmpty()) {
                                onSubmitPassword(password)
                                password = ""
                            }
                        },
                        enabled = password.isNotEmpty() && !state.isLoading,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        if (state.isLoading) {
                            CircularProgressIndicator(
                                modifier = Modifier.height(18.dp),
                                color = MaterialTheme.colorScheme.onPrimary,
                                strokeWidth = 2.dp,
                            )
                        } else {
                            Text("Verify password")
                        }
                    }
                }

                AuthorizationUiStep.Starting,
                AuthorizationUiStep.Ready,
                AuthorizationUiStep.LoggingOut -> {
                    if (state.isLoading) {
                        CircularProgressIndicator()
                    }
                }
                }
            }

            (validationError ?: state.error)?.takeIf(String::isNotBlank)?.let { message ->
                Surface(
                    color = MaterialTheme.colorScheme.errorContainer,
                    contentColor = MaterialTheme.colorScheme.onErrorContainer,
                    shape = MaterialTheme.shapes.medium,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        text = message,
                        modifier = Modifier.padding(16.dp),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }

            if (state.authorization == AuthorizationUiStep.Ready) {
                Text(
                    text = "Authorization state is supplied by the Rust backend.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
