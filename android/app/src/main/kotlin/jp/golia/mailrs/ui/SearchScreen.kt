package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.SearchBar
import androidx.compose.material3.SearchBarDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.UiState
import jp.golia.mailrs.clearSearch
import jp.golia.mailrs.search
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.Wire

/**
 * Finding a message.
 *
 * **The platform's search bar, not a text field in the toolbar.**
 * `SearchBar` is what every Android app that searches uses: it expands
 * over the content, keeps the field pinned while results scroll under
 * it, and takes the back gesture as "collapse" rather than "leave the
 * screen". A `TextField` in the app bar looks similar and behaves like
 * none of that.
 *
 * **The server's order is the order shown.** The endpoint walks ranked
 * hit ids and hydrates them in rank order, so re-sorting by date here
 * would throw the ranking away and lead with the least relevant match.
 * The stub's fixture is built to catch exactly that: both conversations
 * carry "ref 2026" and the older one is returned first.
 *
 * **Nothing matched** and **nothing typed** are different screens. An
 * empty result list is a conclusion — the term is what came back empty —
 * whereas an unopened search has nothing to conclude yet.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MailSearchBar(
    state: UiState,
    vm: MailViewModel,
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
) {
    val theme = LocalTheme.current

    SearchBar(
        expanded = expanded,
        onExpandedChange = onExpandedChange,
        modifier = Modifier.fillMaxWidth().testTag("search.bar"),
        inputField = {
            SearchBarDefaults.InputField(
                query = state.searchTerm,
                onQueryChange = { vm.search(it) },
                onSearch = { vm.search(it) },
                expanded = expanded,
                onExpandedChange = onExpandedChange,
                placeholder = { Text("Search mail", fontSize = 15.sp) },
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null, tint = theme.fgSecondary) },
                trailingIcon = {
                    if (state.searchTerm.isNotEmpty()) {
                        IconButton(
                            onClick = { vm.clearSearch() },
                            modifier = Modifier.testTag("search.clear"),
                        ) {
                            Icon(Icons.Filled.Close, contentDescription = "Clear", tint = theme.fgSecondary)
                        }
                    }
                },
                modifier = Modifier.testTag("search.field"),
            )
        },
    ) {
        SearchResults(state, vm)
    }
}

@Composable
private fun SearchResults(state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val results = state.results

    when {
        state.searching && results == null ->
            Box(Modifier.fillMaxSize()) {
                CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)
            }

        results == null ->
            // Not an empty state: nothing has been asked yet, so there is
            // nothing to report. Saying "no results" before a search is
            // an answer to a question nobody put.
            Box(Modifier.fillMaxSize()) {
                Text(
                    "Search subjects and message text.",
                    color = theme.fgMuted,
                    fontSize = 13.sp,
                    modifier = Modifier.align(Alignment.Center).padding(32.dp),
                )
            }

        results.isEmpty() ->
            Box(Modifier.fillMaxSize()) {
                // The tag goes on the text, not the box: a `Box` does
                // not merge its children's semantics, so a tag on it
                // finds a node with no text to assert against.
                Text(
                    "No mail matches “${state.searchTerm}”.",
                    color = theme.fgMuted,
                    fontSize = 13.sp,
                    modifier = Modifier.align(Alignment.Center).padding(32.dp).testTag("search.empty"),
                )
            }

        else -> LazyColumn(Modifier.fillMaxSize().testTag("list.searchResults")) {
            items(results, key = Wire.Conversation::threadId) { c ->
                ConversationRow(c) { vm.open(c) }
                HorizontalDivider(color = theme.border, thickness = 0.5.dp)
            }
            // **Said, not hidden.** Search has no keyset parameter, so
            // there is no next page to fetch — a full result set is a
            // ceiling, and fifty hits shown as though they were all of
            // them is the same silent truncation the conversation list
            // had. The reader can narrow the term; they cannot do that
            // if nobody tells them.
            if (results.size >= MailrsClient.SEARCH_LIMIT) {
                item {
                    Text(
                        "First ${MailrsClient.SEARCH_LIMIT} matches. Narrow the search to see more.",
                        color = theme.fgMuted,
                        fontSize = 12.sp,
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(16.dp)
                            .testTag("search.capped"),
                    )
                }
            }
        }
    }
}
