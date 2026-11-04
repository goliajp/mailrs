package jp.golia.mailrs.wire

import java.io.File
import jp.golia.mailrs.R
import jp.golia.mailrs.ui.iconOf
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * [RowAction] against `actions.json`, the action table the web reads and
 * the app ships a byte-identical copy of (`scripts/check-action-parity.sh`
 * compares the copies). The enum stays the app's own, typed, code; this
 * says it has not drifted from the shared table, field by field.
 */
class ActionTableTest {
    private val entries: Map<String, JsonObject> by lazy {
        Json.parseToJsonElement(File("src/main/assets/actions.json").readText())
            .jsonObject.getValue("actions").jsonArray
            .map { it.jsonObject }
            .filter { e -> e.getValue("surfaces").jsonArray.any { it.jsonPrimitive.content == "android" } }
            .associateBy { it.getValue("id").jsonPrimitive.content }
    }

    /** `DeleteForever` is the table's `delete-forever`. */
    private fun tableId(action: RowAction): String =
        action.name.replace(Regex("(?<!^)([A-Z])"), "-$1").lowercase()

    /** A string resource id, by the name it has in `strings.xml`. */
    private val stringNames: Map<Int, String> by lazy {
        R.string::class.java.fields.associate { it.getInt(null) to it.name }
    }

    private fun text(e: JsonObject, field: String): String? = when (val v = e.getValue(field)) {
        is JsonNull -> null
        is JsonPrimitive -> v.content
        is JsonObject -> text(v, "android")
        is JsonArray -> error("$field is a list")
    }

    @Test
    fun offers_exactly_the_tables_actions() {
        assertEquals(entries.keys, RowAction.entries.map(::tableId).toSet())
    }

    @Test
    fun every_action_matches_its_entry() {
        for (action in RowAction.entries) {
            val id = tableId(action)
            val e = entries.getValue(id)
            assertEquals("$id confirm", e.getValue("confirm").jsonPrimitive.boolean, action.confirm)
            assertEquals("$id undo", text(e, "undo")?.takeIf { it in entries }, action.undo?.let(::tableId))
            assertEquals("$id label", text(e, "label"), stringNames[action.label])
            assertEquals("$id done", text(e, "done"), action.done?.let { stringNames[it] })
            assertEquals("$id icon", text(e, "icon"), iconOf(action).name)
            assertEquals("$id verb", text(e, "verb"), action.batch)
            assertEquals("$id leaves the list", e.getValue("leaves_list").jsonPrimitive.boolean, action.leavesList)
        }
    }
}
