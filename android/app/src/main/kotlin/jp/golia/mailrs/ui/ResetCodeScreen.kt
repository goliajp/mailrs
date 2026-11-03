package jp.golia.mailrs.ui

import jp.golia.mailrs.R
import jp.golia.mailrs.wire.Texts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.ResetPrompt
import jp.golia.mailrs.wire.contactText

/**
 * Setting a new password on an account whose password an administrator
 * cleared, with the code an issuer handed its owner in person. Nothing is
 * signed in here; the sign-in screen comes back saying to use it.
 */
@Composable
fun ResetCodeScreen(
    prompt: ResetPrompt,
    onSubmit: (address: String, code: String, password: String, repeated: String) -> Unit,
    onCancel: () -> Unit,
) {
    val theme = LocalTheme.current
    var address by rememberSaveable { mutableStateOf(prompt.address) }
    var code by rememberSaveable { mutableStateOf("") }
    // kept out of saved state, as on the sign-in screen
    var password by remember { mutableStateOf("") }
    var repeated by remember { mutableStateOf("") }
    val whom = prompt.contact ?: contactText("", "")
    val ready = !prompt.busy && address.isNotBlank() && code.isNotBlank() && password.isNotEmpty()

    Column(
        Modifier
            .fillMaxSize()
            .background(theme.bg)
            .verticalScroll(rememberScrollState())
            .imePadding()
            .padding(horizontal = 24.dp)
            .padding(top = 56.dp, bottom = 24.dp),
    ) {
        Text(Texts.get(R.string.reset_title), color = theme.fg, fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
        Text(
            Texts.get(R.string.reset_body, whom),
            color = theme.fgMuted,
            fontSize = 14.sp,
            modifier = Modifier.padding(top = 8.dp, bottom = 16.dp).testTag("text.resetExplain"),
        )
        OutlinedTextField(
            value = address,
            onValueChange = { address = it },
            label = { Text(Texts.get(R.string.common_address)) },
            singleLine = true,
            keyboardOptions = addressKeyboard(),
            modifier = Modifier.fillMaxWidth().testTag("field.resetAddress"),
        )
        OutlinedTextField(
            value = code,
            onValueChange = { code = it },
            label = { Text(Texts.get(R.string.reset_code)) },
            supportingText = { Text(Texts.get(R.string.reset_code_hint)) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Ascii),
            modifier = Modifier.fillMaxWidth().padding(top = 10.dp).testTag("field.resetCode"),
        )
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text(Texts.get(R.string.reset_new_password)) },
            supportingText = { Text(Texts.get(R.string.reset_password_hint)) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            modifier = Modifier.fillMaxWidth().padding(top = 4.dp).testTag("field.resetPassword"),
        )
        OutlinedTextField(
            value = repeated,
            onValueChange = { repeated = it },
            label = { Text(Texts.get(R.string.reset_new_password_again)) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            modifier = Modifier.fillMaxWidth().padding(top = 4.dp).testTag("field.resetRepeat"),
        )
        prompt.failure?.let {
            Text(
                it,
                color = theme.danger,
                fontSize = 13.sp,
                modifier = Modifier.fillMaxWidth().padding(top = 10.dp).testTag("text.resetFailure"),
            )
        }
        Button(
            onClick = { onSubmit(address, code, password, repeated) },
            enabled = ready,
            colors = ButtonDefaults.buttonColors(containerColor = theme.accent, contentColor = theme.accentFg),
            shape = RoundedCornerShape(10.dp),
            modifier = Modifier.fillMaxWidth().padding(top = 20.dp).height(48.dp).testTag("button.resetSubmit"),
        ) {
            Text(Texts.get(R.string.reset_set), fontWeight = FontWeight.SemiBold)
        }
        TextButton(onClick = onCancel, modifier = Modifier.fillMaxWidth().testTag("button.resetCancel")) {
            Text(Texts.get(R.string.reset_back_to_sign_in), color = theme.accent)
        }
    }
}
