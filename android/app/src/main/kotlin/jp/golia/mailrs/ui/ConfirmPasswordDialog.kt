package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.ConfirmPrompt
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.cancelConfirmation
import jp.golia.mailrs.submitConfirmation

/**
 * The password prompt before a change the server asked to confirm —
 * adding an alias, a group member, an agent key. Shown over whatever
 * screen asked; the change goes through once the server accepts.
 */
@Composable
fun ConfirmPasswordDialog(prompt: ConfirmPrompt, vm: MailViewModel) {
    val theme = LocalTheme.current
    var password by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    val ready = password.isNotEmpty() && !prompt.busy
    fun submit() {
        if (ready) vm.submitConfirmation(password, code)
    }
    AlertDialog(
        onDismissRequest = { vm.cancelConfirmation() },
        containerColor = theme.surface,
        title = { Text("Confirm it’s you", fontSize = 16.sp, color = theme.fg) },
        text = {
            Column {
                Text(
                    "Enter your password to continue. You won’t be asked again for 10 minutes.",
                    fontSize = 13.sp,
                    color = theme.fgMuted,
                    modifier = Modifier.padding(bottom = 8.dp),
                )
                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    label = { Text("Password", fontSize = 13.sp) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Password,
                        imeAction = imeAfterPassword(prompt.needsCode),
                    ),
                    keyboardActions = KeyboardActions(onDone = { submit() }),
                    modifier = Modifier.fillMaxWidth().testTag("field.confirmPassword"),
                )
                if (prompt.needsCode) {
                    OutlinedTextField(
                        value = code,
                        onValueChange = { code = it },
                        label = { Text("Authenticator code", fontSize = 13.sp) },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(
                            keyboardType = KeyboardType.NumberPassword,
                            imeAction = ImeAction.Done,
                        ),
                        keyboardActions = KeyboardActions(onDone = { submit() }),
                        modifier = Modifier.fillMaxWidth().testTag("field.confirmCode"),
                    )
                }
                prompt.message?.let {
                    Text(
                        it,
                        fontSize = 13.sp,
                        color = theme.danger,
                        modifier = Modifier
                            .padding(top = 8.dp)
                            .testTag("text.confirmMessage")
                            .semantics { liveRegion = LiveRegionMode.Polite },
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = ready,
                onClick = { submit() },
                modifier = Modifier.testTag("button.confirmPassword"),
            ) {
                Text("Continue", color = theme.accent)
            }
        },
        dismissButton = {
            TextButton(
                onClick = { vm.cancelConfirmation() },
                modifier = Modifier.testTag("button.cancelConfirm"),
            ) { Text("Cancel", color = theme.fgSecondary) }
        },
    )
}

private fun imeAfterPassword(needsCode: Boolean): ImeAction = when (needsCode) {
    true -> ImeAction.Next
    false -> ImeAction.Done
}
