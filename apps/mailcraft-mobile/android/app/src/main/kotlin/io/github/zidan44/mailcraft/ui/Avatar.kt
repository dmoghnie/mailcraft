package io.github.zidan44.mailcraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.zidan44.mailcraft.Avatar

/**
 * A person's or an account's initials on their own colour (the core picks the hue from the
 * address); a tick in its place when the row is selected.
 */
@Composable
fun AvatarCircle(avatar: Avatar, size: Dp = 40.dp, selected: Boolean = false, modifier: Modifier = Modifier) {
    val dark = isSystemInDarkTheme()
    val hue = avatar.hue.coerceIn(0, 359).toFloat()
    val back = if (selected) MaterialTheme.colorScheme.primary else Color.hsl(hue, 0.45f, if (dark) 0.35f else 0.82f)
    val fore = if (selected) MaterialTheme.colorScheme.onPrimary else Color.hsl(hue, 0.6f, if (dark) 0.9f else 0.25f)
    Box(modifier.size(size).clip(CircleShape).background(back), contentAlignment = Alignment.Center) {
        if (selected) {
            Icon(Symbols.Check, "Selected", tint = fore)
        } else {
            Text(avatar.initials, color = fore, fontWeight = FontWeight.SemiBold, fontSize = (size.value * 0.4f).sp)
        }
    }
}
