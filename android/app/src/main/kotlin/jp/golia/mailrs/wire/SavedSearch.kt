package jp.golia.mailrs.wire

import kotlinx.serialization.Serializable

/**
 * A search the reader saved on the web. The phone runs them; making and
 * deleting them stays on the web. The same type as the iPhone's
 * `Wire/SavedSearch.swift`.
 * Backend: `core-sidestate/families/saved_searches.rs` — `SavedSearch`.
 */
@Serializable
data class SavedSearch(val id: String, val name: String, val query: String)

suspend fun MailrsClient.savedSearches() = decode(get("/api/saved-searches"), SavedSearch.serializer())
