package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import jp.golia.mailrs.R
import jp.golia.mailrs.wire.Texts

/**
 * Grey rows standing in for a list whose first page is still out.
 *
 * Laid out like the real rows from the top of the list, not a spinner in
 * the middle: the screen keeps its shape from the switch to the answer,
 * so nothing moves when the rows arrive.
 */
@Composable
internal fun ListPlaceholder(modifier: Modifier = Modifier) {
    val loading = Texts.get(R.string.common_loading)
    Column(
        modifier
            .fillMaxSize()
            .semantics { contentDescription = loading }
            .testTag("list.loading"),
    ) {
        repeat(ROWS) { index ->
            PlaceholderRow(index)
            if (index < ROWS - 1) {
                HorizontalDivider(color = LocalTheme.current.border, thickness = 0.5.dp)
            }
        }
    }
}

private const val ROWS = 8

/** Subject bars of a few lengths, so the block reads as different mail. */
private val SUBJECT_WIDTHS = listOf(0.82f, 0.58f, 0.72f, 0.46f)

@Composable
private fun PlaceholderRow(index: Int) {
    val fill = LocalTheme.current.bgTertiary
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 6.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Box(Modifier.size(36.dp).background(fill, CircleShape))
        Column(Modifier.padding(start = 12.dp).weight(1f)) {
            // each bar sits in a box one text line tall, so the rows are
            // as tall as the real ones: 15sp name, 14sp subject, 12sp snippet
            Row(verticalAlignment = Alignment.CenterVertically) {
                Bar(Modifier.width(96.dp), line = 20.dp)
                Spacer(Modifier.weight(1f))
                Bar(Modifier.width(36.dp), line = 20.dp)
            }
            Bar(Modifier.fillMaxWidth(SUBJECT_WIDTHS[index % SUBJECT_WIDTHS.size]), line = 20.dp)
            Bar(Modifier.fillMaxWidth(0.66f), line = 16.dp)
        }
    }
}

@Composable
private fun Bar(modifier: Modifier, line: Dp) {
    Box(Modifier.height(line), contentAlignment = Alignment.CenterStart) {
        Box(modifier.height(10.dp).background(LocalTheme.current.bgTertiary, RoundedCornerShape(3.dp)))
    }
}
