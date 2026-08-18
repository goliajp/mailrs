package jp.golia.mailrs.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.foundation.background
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.semantics.contentType
import androidx.compose.ui.autofill.ContentType
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The sign-in screen is a front door.
 *
 * `ios/DESIGN.md`: the mark, the name, one line of what this is, and a
 * full-width prominent button — *"not another table row"*. The first
 * Android draft was three bare fields and a text button, which is the
 * table row.
 *
 * Typing belongs at the top: the fields sit in the upper half so the
 * keyboard does not cover the thing the reader came to type into.
 */
@Composable
fun SignInScreen(busy: Boolean, error: String?, onSignIn: (String, String, String) -> Unit) {
    val theme = LocalTheme.current
    // **Saveable, so a rotation does not empty the form** — typing an
    // address and turning the phone should not start it again.
    var server by rememberSaveable { mutableStateOf("mail.golia.jp") }
    var username by rememberSaveable { mutableStateOf("") }
    // **Except the password.** Saved state is a Bundle, and a Bundle
    // goes to disk in the saved instance state; a password that
    // survives a rotation by being written there is a password on the
    // device. Retyping it is the cheaper cost.
    var password by remember { mutableStateOf("") }

    val ready = !busy && username.isNotBlank() && password.isNotBlank()
    fun submit() {
        if (ready) onSignIn(server, username, password)
    }

    Column(
        Modifier
            .fillMaxSize()
            .background(theme.bg)
            // Edge to edge puts this under the keyboard as well as under
            // the system bars: without these the Sign in button sits
            // beneath the IME on a short screen, and the only way to
            // reach it is to dismiss the keyboard first.
            .verticalScroll(rememberScrollState())
            .imePadding()
            .padding(horizontal = 24.dp)
            .padding(top = 72.dp, bottom = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        AppMark(size = 64.dp)
        Text(
            "Mailrs",
            color = theme.fg,
            fontSize = 30.sp,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.padding(top = 14.dp),
        )
        Text(
            "Your own mail server.",
            color = theme.fgMuted,
            fontSize = 14.sp,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 4.dp, bottom = 28.dp),
        )

        OutlinedTextField(
            value = server,
            onValueChange = { server = it },
            label = { Text("Server") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Uri,
                imeAction = ImeAction.Next,
            ),
            modifier = Modifier.fillMaxWidth().testTag("field.server"),
        )
        OutlinedTextField(
            value = username,
            onValueChange = { username = it },
            label = { Text("Address") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Email,
                imeAction = ImeAction.Next,
            ),
            // Named to the autofill service, so a password manager can
            // offer the saved account rather than the reader retyping an
            // address into a phone keyboard.
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 10.dp)
                .semantics { contentType = ContentType.EmailAddress }
                .testTag("field.address"),
        )
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text("Password") },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                // The last field, so the keyboard offers Go rather than
                // a newline — and it does what the button does.
                imeAction = ImeAction.Go,
            ),
            keyboardActions = KeyboardActions(onGo = { submit() }),
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 10.dp)
                .semantics { contentType = ContentType.Password }
                .testTag("field.password"),
        )

        if (error != null) {
            Text(
                error,
                color = theme.danger,
                fontSize = 13.sp,
                modifier = Modifier.fillMaxWidth().padding(top = 10.dp).testTag("text.signInError"),
            )
        }

        Button(
            onClick = { submit() },
            enabled = ready,
            colors = ButtonDefaults.buttonColors(containerColor = theme.accent, contentColor = theme.accentFg),
            shape = RoundedCornerShape(10.dp),
            modifier = Modifier.fillMaxWidth().padding(top = 20.dp).height(48.dp).testTag("button.signIn"),
        ) {
            Text(if (busy) "Signing in…" else "Sign in", fontWeight = FontWeight.SemiBold)
        }
    }
}
