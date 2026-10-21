package jp.golia.mailrs.ui

import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.intl.LocaleList

/**
 * The keyboard for a field that takes an address or a host name.
 *
 * The keyboard type alone is not enough: a Chinese or Japanese input
 * method keeps composing in an email field, so `qa@golia.jp` comes out as
 * characters. The English locale hint is what makes it switch to Latin
 * input, and an address is never corrected or capitalised.
 */
fun addressKeyboard(
    imeAction: ImeAction = ImeAction.Next,
    keyboardType: KeyboardType = KeyboardType.Email,
): KeyboardOptions = KeyboardOptions(
    keyboardType = keyboardType,
    imeAction = imeAction,
    capitalization = KeyboardCapitalization.None,
    autoCorrectEnabled = false,
    hintLocales = LocaleList("en"),
)
