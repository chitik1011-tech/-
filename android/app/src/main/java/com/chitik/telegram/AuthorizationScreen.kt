package com.chitik.telegram

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
    onStart: () -> Unit,
    onSubmitPhoneNumber: (String) -> Unit,
    onSubmitCode: (String) -> Unit,
    onSubmitPassword: (String) -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var phoneNumber by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    val focusRequester = remember(state.authorization) { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    val focusableStep = state.authorization == AuthorizationUiStep.WaitingForPhoneNumber ||
        state.authorization == AuthorizationUiStep.WaitingForCode ||
        state.authorization == AuthorizationUiStep.WaitingForPassword

    BackHandler(enabled = true) {
        keyboard?.hide()
        onBack()
    }

    LaunchedEffect(state.authorization, state.isLoading) {
        if (focusableStep && !state.isLoading) {
            focusRequester.requestFocus()
            keyboard?.show()
        } else {
            keyboard?.hide()
        }
    }

    val title = when (state.authorization) {
        AuthorizationUiStep.Starting -> "Connecting to Telegram"
        AuthorizationUiStep.WaitingForPhoneNumber -> "Your phone number"
        AuthorizationUiStep.WaitingForCode -> "Enter the login code"
        AuthorizationUiStep.WaitingForPassword -> "Two-step verification"
        AuthorizationUiStep.Ready -> "You're signed in"
        AuthorizationUiStep.LoggingOut -> "Signing out"
        AuthorizationUiStep.Closed -> "Sign in to Telegram"
    }
    val subtitle = when (state.authorization) {
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
            .padding(horizontal = 24.dp, vertical = 32.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(16.dp),
            horizontalAlignment = Alignment.Start,
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.headlineMedium,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                text = subtitle,
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            when (state.authorization) {
                AuthorizationUiStep.Closed -> {
                    Button(
                        onClick = onStart,
                        enabled = !state.isLoading,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        if (state.isLoading) {
                            CircularProgressIndicator(
                                modifier = Modifier.height(18.dp),
                                color = MaterialTheme.colorScheme.onPrimary,
                                strokeWidth = 2.dp,
                            )
                            Spacer(Modifier.height(8.dp))
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
                            .focusRequester(focusRequester),
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
                            .focusRequester(focusRequester),
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
                            .focusRequester(focusRequester),
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

            state.error?.takeIf(String::isNotBlank)?.let { message ->
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
