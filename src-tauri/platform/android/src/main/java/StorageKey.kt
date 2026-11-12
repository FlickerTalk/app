package com.flickertalk.platform

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import app.tauri.annotation.InvokeArg
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

private const val KEY_ALIAS = "ft.storage"
private const val IV_BYTES = 12

// GCM nonce, then ciphertext + tag
fun splitSealed(sealed: ByteArray): Pair<ByteArray, ByteArray>? =
    if (sealed.size <= IV_BYTES) null else Pair(sealed.copyOfRange(0, IV_BYTES), sealed.copyOfRange(IV_BYTES, sealed.size))

// made once, never leaves the Keystore
private fun keystoreKey(): SecretKey {
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (store.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generator.init(
        KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .build()
    )
    return generator.generateKey()
}

@InvokeArg
class KeyArgs {
    lateinit var value: String
}

fun sealStorageKey(key: ByteArray): ByteArray {
    val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, keystoreKey()) }
    return cipher.iv + cipher.doFinal(key)
}

fun openStorageKey(sealed: ByteArray): ByteArray {
    val (iv, ciphertext) = splitSealed(sealed) ?: throw IllegalArgumentException("not a sealed key")
    val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply {
        init(Cipher.DECRYPT_MODE, keystoreKey(), GCMParameterSpec(128, iv))
    }
    return cipher.doFinal(ciphertext)
}

fun forgetStorageKey() {
    KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.deleteEntry(KEY_ALIAS)
}
