package jp.golia.mailrs

import jp.golia.mailrs.wire.Role
import org.junit.Assert.assertEquals
import org.junit.Test

class AdminSectionOfferTest {
    private fun offered(role: Role?) = AdminSection.entries.filter { it.offeredTo(role) }.toSet()

    private val own = setOf(AdminSection.AgentKeys, AdminSection.Allowed, AdminSection.Blocked)
    private val othersMail = setOf(AdminSection.Queue, AdminSection.Audit, AdminSection.Suppressed)

    @Test
    fun rights_not_yet_known_offer_only_the_accounts_own_pages() {
        assertEquals(own, offered(null))
    }

    @Test
    fun an_account_that_is_not_an_administrator_gets_its_own_pages() {
        assertEquals(own, offered(Role(isAdmin = false, isSuper = false)))
    }

    @Test
    fun an_administrator_gets_everything_but_other_peoples_mail_traffic() {
        assertEquals(AdminSection.entries.toSet() - othersMail, offered(Role(isAdmin = true, isSuper = false)))
    }

    @Test
    fun a_super_administrator_gets_every_page() {
        assertEquals(AdminSection.entries.toSet(), offered(Role(isAdmin = true, isSuper = true)))
    }
}
