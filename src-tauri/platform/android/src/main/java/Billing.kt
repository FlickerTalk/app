package com.flickertalk.platform

import android.app.Activity
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import com.android.billingclient.api.AcknowledgePurchaseParams
import com.android.billingclient.api.BillingClient
import com.android.billingclient.api.BillingClientStateListener
import com.android.billingclient.api.BillingFlowParams
import com.android.billingclient.api.BillingResult
import com.android.billingclient.api.PendingPurchasesParams
import com.android.billingclient.api.ProductDetails
import com.android.billingclient.api.Purchase
import com.android.billingclient.api.QueryProductDetailsParams
import com.android.billingclient.api.QueryPurchasesParams

// product and base plan ids as set up in the Play Console
const val YEARLY = "yearly"

const val YEARLY_BASE_PLAN = "yearly-autorenew"

data class StoreOffer(
    val basePlan: String,
    val offerId: String?,
    val token: String,
    val prices: List<String>,
)

fun yearlyOffer(offers: List<StoreOffer>): StoreOffer? {
    val yearly = offers.filter { it.basePlan == YEARLY_BASE_PLAN }
    return yearly.firstOrNull { it.offerId == null } ?: yearly.firstOrNull()
}

fun yearlyPrice(offers: List<StoreOffer>): String? =
    yearlyOffer(offers)?.prices?.lastOrNull()?.trim()?.takeIf { it.isNotEmpty() }

data class StorePurchase(
    val product: String,
    val state: Int,
    val boughtAt: Long,
    val acknowledged: Boolean,
)

const val PLAY_CACHE_MS = 365L * 24 * 60 * 60 * 1000

// Play never gives the client the expiry and the purchase time stays the original one on renewals,
// so a listed subscription counts as a year from this check, refreshed on every check.
fun activeUntil(purchases: List<StorePurchase>, now: Long): Long =
    if (purchases.any { it.product == YEARLY && it.state == Purchase.PurchaseState.PURCHASED }) {
        now + PLAY_CACHE_MS
    } else {
        0L
    }

fun purchaseTrouble(purchases: List<StorePurchase>): String? =
    if (activeUntil(purchases, 0L) == 0L &&
        purchases.any { it.product == YEARLY && it.state == Purchase.PurchaseState.PENDING }
    ) {
        "pending_approval"
    } else {
        null
    }

// Google refunds a purchase that isn't acknowledged within three days
fun needsAcknowledgement(purchase: StorePurchase): Boolean =
    purchase.state == Purchase.PurchaseState.PURCHASED && !purchase.acknowledged

class EntitlementQueue {
    private var sink: ((Long) -> Unit)? = null
    private var latest: Long? = null

    @Synchronized
    fun register(sink: (Long) -> Unit) {
        this.sink = sink
        latest?.let {
            latest = null
            sink(it)
        }
    }

    @Synchronized
    fun offer(until: Long) {
        val sink = sink
        if (sink != null) sink(until) else latest = until
    }
}

object EntitlementEvents {
    private val queue = EntitlementQueue()
    private val sender = java.util.concurrent.Executors.newSingleThreadExecutor()

    fun register(channel: Channel) = sender.execute {
        queue.register { channel.sendObject(mapOf("until" to it)) }
    }

    fun offer(until: Long) = sender.execute { queue.offer(until) }
}

// rejections are plan.trouble.* keys, the app translates them
class Billing(private val activity: Activity) {
    private var billing: BillingClient? = null

    private var buying: Invoke? = null

    private fun withBilling(invoke: Invoke, work: (BillingClient) -> Unit) =
        withBilling({ invoke.reject("store_unavailable") }, work)

    private fun withBilling(unavailable: () -> Unit, work: (BillingClient) -> Unit) {
        val client = billing ?: BillingClient.newBuilder(activity)
            .enableAutoServiceReconnection()
            .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
            .setListener { result, purchases -> purchasesUpdated(result, purchases) }
            .build()
            .also { billing = it }
        if (client.isReady) {
            work(client)
            return
        }
        client.startConnection(object : BillingClientStateListener {
            override fun onBillingSetupFinished(result: BillingResult) {
                if (result.responseCode == BillingClient.BillingResponseCode.OK) {
                    work(client)
                } else {
                    unavailable()
                }
            }

            override fun onBillingServiceDisconnected() {}
        })
    }

    private fun storePurchases(purchases: List<Purchase>): List<StorePurchase> =
        purchases.flatMap { purchase ->
            purchase.products.map {
                StorePurchase(it, purchase.purchaseState, purchase.purchaseTime, purchase.isAcknowledged)
            }
        }

    private fun acknowledge(client: BillingClient, purchases: List<Purchase>) {
        for (purchase in purchases) {
            val ours = storePurchases(listOf(purchase)).filter { it.product == YEARLY }
            if (ours.any { needsAcknowledgement(it) }) {
                client.acknowledgePurchase(
                    AcknowledgePurchaseParams.newBuilder().setPurchaseToken(purchase.purchaseToken).build(),
                ) {}
            }
        }
    }

    private fun purchasesUpdated(result: BillingResult, purchases: List<Purchase>?) {
        val invoke = buying
        if (invoke == null) {
            if (result.responseCode == BillingClient.BillingResponseCode.OK) {
                billing?.let { acknowledge(it, purchases.orEmpty()) }
                tellEntitlement()
            }
            return
        }
        buying = null
        when (result.responseCode) {
            BillingClient.BillingResponseCode.OK -> {
                val bought = purchases.orEmpty()
                billing?.let { acknowledge(it, bought) }
                val trouble = purchaseTrouble(storePurchases(bought))
                if (trouble != null) {
                    invoke.reject(trouble)
                } else {
                    invoke.resolve(JSObject().apply { put("until", activeUntil(storePurchases(bought), System.currentTimeMillis())) })
                }
            }
            BillingClient.BillingResponseCode.USER_CANCELED -> invoke.reject("cancelled")
            else -> invoke.reject("payment_failed")
        }
    }

    private fun yearlyQuery(): QueryProductDetailsParams =
        QueryProductDetailsParams.newBuilder()
            .setProductList(
                listOf(
                    QueryProductDetailsParams.Product.newBuilder()
                        .setProductId(YEARLY)
                        .setProductType(BillingClient.ProductType.SUBS)
                        .build(),
                ),
            )
            .build()

    private fun storeOffers(details: ProductDetails?): List<StoreOffer> =
        details?.subscriptionOfferDetails.orEmpty().map { offer ->
            StoreOffer(
                offer.basePlanId,
                offer.offerId,
                offer.offerToken,
                offer.pricingPhases.pricingPhaseList.map { it.formattedPrice },
            )
        }

    fun subscriptionPrice(invoke: Invoke) {
        withBilling(invoke) { client ->
            client.queryProductDetailsAsync(yearlyQuery()) { result, found ->
                val details = found.productDetailsList.firstOrNull { it.productId == YEARLY }
                val price = if (result.responseCode == BillingClient.BillingResponseCode.OK) {
                    yearlyPrice(storeOffers(details))
                } else {
                    null
                }
                invoke.resolve(JSObject().apply { price?.let { put("price", it) } })
            }
        }
    }

    fun subscribe(invoke: Invoke) {
        withBilling(invoke) { client ->
            client.queryProductDetailsAsync(yearlyQuery()) { result, found ->
                val details = found.productDetailsList.firstOrNull()
                val offer = yearlyOffer(storeOffers(details))?.token
                    ?: details?.subscriptionOfferDetails?.firstOrNull()?.offerToken
                if (result.responseCode != BillingClient.BillingResponseCode.OK || details == null || offer == null) {
                    invoke.reject("not_on_sale")
                    return@queryProductDetailsAsync
                }
                buying = invoke
                val flow = BillingFlowParams.newBuilder()
                    .setProductDetailsParamsList(
                        listOf(
                            BillingFlowParams.ProductDetailsParams.newBuilder()
                                .setProductDetails(details)
                                .setOfferToken(offer)
                                .build(),
                        ),
                    )
                    .build()
                // launchBillingFlow has to run on the activity's thread
                activity.runOnUiThread {
                    if (client.launchBillingFlow(activity, flow).responseCode != BillingClient.BillingResponseCode.OK) {
                        buying = null
                        invoke.reject("store_unavailable")
                    }
                }
            }
        }
    }

    private fun queryEntitlement(answer: (Long?) -> Unit) {
        withBilling({ answer(null) }) { client ->
            val subs = QueryPurchasesParams.newBuilder()
                .setProductType(BillingClient.ProductType.SUBS)
                .build()
            client.queryPurchasesAsync(subs) { result, purchases ->
                if (result.responseCode != BillingClient.BillingResponseCode.OK) {
                    answer(null)
                    return@queryPurchasesAsync
                }
                acknowledge(client, purchases)
                answer(activeUntil(storePurchases(purchases), System.currentTimeMillis()))
            }
        }
    }

    fun subscription(invoke: Invoke) {
        queryEntitlement { until ->
            if (until == null) {
                invoke.reject("store_unavailable")
            } else {
                invoke.resolve(JSObject().apply { put("until", until) })
            }
        }
    }

    fun tellEntitlement() {
        queryEntitlement { until -> until?.let { EntitlementEvents.offer(it) } }
    }
}
