package jp.golia.mailrs.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.UiState
import jp.golia.mailrs.MailViewModel

/**
 * The message as it arrived.
 *
 * The Received chain, the authentication results, the exact
 * Content-Type — what the operator of a mail server reaches for when a
 * message did not do what it should have, and the one thing nothing
 * else in this app shows.
 *
 * **Monospace, and it scrolls both ways.** A header that wrapped would
 * be a different header: folding is significant in RFC 5322, and a
 * reader deciding whether a line was folded cannot be shown one that
 * this screen folded for them.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SourceScreen(state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    Scaffold(
        containerColor = theme.bg,
        topBar = {
            TopAppBar(
                title = { Text("Source", fontSize = 17.sp, fontWeight = FontWeight.SemiBold) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = theme.bg,
                    titleContentColor = theme.fg,
                ),
                navigationIcon = {
                    IconButton(
                        onClick = { vm.closeSource() },
                        modifier = Modifier.testTag("button.closeSource"),
                    ) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                            tint = theme.fgSecondary,
                        )
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            val source = state.source
            if (source == null) {
                CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)
            } else {
                Text(
                    source,
                    color = theme.fg,
                    fontSize = 11.sp,
                    fontFamily = FontFamily.Monospace,
                    softWrap = false,
                    modifier = Modifier
                        .fillMaxSize()
                        .verticalScroll(rememberScrollState())
                        .horizontalScroll(rememberScrollState())
                        .padding(12.dp)
                        .testTag("text.source"),
                )
            }
        }
    }
}
