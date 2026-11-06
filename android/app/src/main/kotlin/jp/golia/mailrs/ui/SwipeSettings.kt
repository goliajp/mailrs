package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.R
import jp.golia.mailrs.wire.SwipeChoice
import jp.golia.mailrs.wire.SwipeSetting
import jp.golia.mailrs.wire.Texts

/**
 * What a swipe on a row does, per direction. Saved on the server, so the
 * web and the iPhone follow the same choice.
 */
@Composable
fun SwipeSettings(swipe: SwipeSetting, onChange: (SwipeSetting) -> Unit) {
    SwipeChoiceRow(R.string.swipe_right, "right", swipe.right) { onChange(swipe.copy(right = it)) }
    SwipeChoiceRow(R.string.swipe_left, "left", swipe.left) { onChange(swipe.copy(left = it)) }
    Text(
        Texts.get(R.string.swipe_help),
        color = LocalTheme.current.fgMuted,
        fontSize = 12.sp,
        modifier = Modifier.padding(horizontal = 20.dp, vertical = 4.dp),
    )
}

@Composable
private fun SwipeChoiceRow(title: Int, side: String, chosen: SwipeChoice, onPick: (SwipeChoice) -> Unit) {
    val theme = LocalTheme.current
    var open by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(start = 20.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(Texts.get(title), color = theme.fg, fontSize = 14.sp, modifier = Modifier.weight(1f))
        Box {
            TextButton(onClick = { open = true }, modifier = Modifier.testTag("swipe.$side")) {
                Text(Texts.get(chosen.label), fontSize = 14.sp)
            }
            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                for (choice in SwipeChoice.entries) {
                    DropdownMenuItem(
                        text = { Text(Texts.get(choice.label)) },
                        onClick = {
                            open = false
                            onPick(choice)
                        },
                        modifier = Modifier.testTag("swipe.$side.${choice.name}"),
                    )
                }
            }
        }
    }
}
