package jp.golia.mailrs.ui

import jp.golia.mailrs.R
import jp.golia.mailrs.wire.Texts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
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
                placeholder = { Text(Texts.get(R.string.search_placeholder), fontSize = 15.sp) },
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null, tint = theme.fgSecondary) },
                trailingIcon = {
                    if (state.searchTerm.isNotEmpty()) {
                        IconButton(
                            onClick = { vm.clearSearch() },
                            modifier = Modifier.testTag("search.clear"),
                        ) {
                            Icon(Icons.Filled.Close, contentDescription = Texts.get(R.string.common_clear), tint = theme.fgSecondary)
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

        // Not an empty state: nothing has been asked yet, so there is
        // nothing to report. Saying "no results" before a search is an
        // answer to a question nobody put. What is offered instead is
        // what the web offers: the saved searches and the operators.
        results == null -> SavedSearches(state, vm)

        results.isEmpty() ->
            Box(Modifier.fillMaxSize()) {
                // The tag goes on the text, not the box: a `Box` does
                // not merge its children's semantics, so a tag on it
                // finds a node with no text to assert against.
                Text(
                    Texts.get(R.string.search_no_match, state.searchTerm),
                    color = theme.fgMuted,
                    fontSize = 13.sp,
                    modifier = Modifier.align(Alignment.Center).padding(32.dp).testTag("search.empty"),
                )
            }

        else -> LazyColumn(Modifier.fillMaxSize().testTag("list.searchResults")) {
            itemsIndexed(results, key = { _, c -> c.threadId }) { index, c ->
                ConversationRow(c, state.myAddress) { vm.open(c) }
                if (index < results.lastIndex) {
                    HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                }
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
                        Texts.get(R.string.search_first_n, MailrsClient.SEARCH_LIMIT),
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

/** The searches saved on the web, each running its query, then the operators the server takes. */
@Composable
private fun SavedSearches(state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    LazyColumn(Modifier.fillMaxSize().testTag("search.savedList")) {
        if (state.savedSearches.isNotEmpty()) {
            item {
                Text(
                    Texts.get(R.string.search_saved),
                    color = theme.fgSecondary,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 4.dp),
                )
            }
        }
        items(state.savedSearches, key = { it.id }) { saved ->
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { vm.search(saved.query) }
                    .padding(horizontal = 16.dp, vertical = 12.dp)
                    .testTag("search.saved.${saved.id}"),
            ) {
                Icon(Icons.Filled.Search, contentDescription = null, tint = theme.fgSecondary, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(12.dp))
                Text(saved.name, color = theme.fg, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        item {
            Text(
                Texts.get(R.string.search_hint),
                color = theme.fgMuted,
                fontSize = 13.sp,
                modifier = Modifier.fillMaxWidth().padding(16.dp).testTag("search.operators"),
            )
        }
    }
}
